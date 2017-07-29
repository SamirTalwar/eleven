extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate slog_json;

use slog::Drain;
use std::clone::Clone;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::io::prelude::*;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Mutex;

const NOT_FOUND: &'static str = "{\"status\":404,\"body\":\"\"}\n";


fn main() {
    let json = slog_json::Json::new(std::io::stderr())
        .add_key_value(o!(
                "level" => slog::FnValue(move |record: &slog::Record| {
                    record.level().as_short_str()
                }),
                "msg" => slog::PushFnValue(move |record: &slog::Record, serializer| {
                    serializer.emit(record.msg())
                }),
                ))
        .build();
    let drain = Mutex::new(json).map(slog::Fuse);
    let logger = slog::Logger::root(drain, o!("service" => "router"));

    match run(&logger) {
        Ok(_) => {}
        Err(error) => error!(logger, "Error: {}", error),
    }
}

fn run(logger: &slog::Logger) -> io::Result<()> {
    let incoming_socket = &env::args()
                               .nth(1)
                               .ok_or(io::Error::new(io::ErrorKind::Other,
                                                     "No argument provided.".to_string()))?;
    let configuration = read_configuration()?;

    let listener = UnixListener::bind(&Path::new(incoming_socket))?;
    for c in listener.incoming() {
        let mut connection = c?;
        let mut input = String::new();
        connection.read_to_string(&mut input)?;
        match handle(&configuration.routes, &logger, &input) {
            Ok(response) => {
                connection.write_fmt(format_args!("{}", &response))?;
            }
            Err(error) => {
                error!(logger, "Connection error: {}", &error);
                connection.write(b"{}")?;
            }
        }
    }
    Ok(())
}

fn handle(routes: &Vec<Route>, logger: &slog::Logger, input: &String) -> io::Result<String> {
    info!(logger, "request"; "request" => &input);
    let incoming_request: IncomingHttpRequest = parse_json(&input.trim().to_string())?;
    let result = routes
        .iter()
        .filter_map(|route| route.matching(&incoming_request))
        .next();
    let output = match result {
        Some((route, outgoing_request)) => route.handle(&outgoing_request),
        None => Ok(NOT_FOUND.to_string()),
    }?;
    info!(logger, "response"; "response" => &output);
    Ok(output)
}

fn read_configuration() -> io::Result<Configuration> {
    let arg =
        env::args()
            .nth(2)
            .ok_or(io::Error::new(io::ErrorKind::Other, "No argument provided.".to_string()))?;
    let file_path = &Path::new(&arg);
    let mut file = File::open(file_path)?;
    let mut string = String::new();
    file.read_to_string(&mut string)?;
    parse_json(&string)
}

fn parse_json<T>(input: &String) -> io::Result<T>
    where T: serde::Deserialize
{
    as_io_error(serde_json::from_str(&input))
}

fn write_json<W, T: ?Sized>(writer: &mut W, value: &T) -> io::Result<()>
    where W: Write,
          T: serde::Serialize
{
    as_io_error(serde_json::to_writer(writer, value))
}

fn as_io_error<T>(result: serde_json::Result<T>) -> io::Result<T> {
    result.map_err(|error| io::Error::new(io::ErrorKind::Other, error))
}

#[derive(Serialize, Deserialize)]
struct Configuration {
    routes: Vec<Route>,
}

#[derive(Serialize, Deserialize)]
struct Route {
    method: HttpMethod,
    path: RoutePath,
    process: String,
}

impl Route {
    fn matching(&self, request: &IncomingHttpRequest) -> Option<(&Route, OutgoingHttpRequest)> {
        if self.method != request.method {
            return None;
        }

        let segments: Vec<String> = request
            .path
            .split("/")
            .map(|s| s.to_string())
            .collect();
        if segments.len() != self.path.0.len() {
            return None;
        }

        let zipped: Vec<(String, &RoutePathSegment)> =
            segments.into_iter().zip(&self.path.0).collect();

        let matches = zipped
            .iter()
            .all(|&(ref request_segment, &ref route_segment)| match route_segment {
                     &RoutePathSegment::Fixed(ref f) => request_segment == f,
                     &RoutePathSegment::Variable(_) => true,
                 });
        if !matches {
            return None;
        }

        let path_params = zipped
            .into_iter()
            .filter_map(move |(request_segment, route_segment)| match *route_segment {
                            RoutePathSegment::Fixed(_) => None,
                            RoutePathSegment::Variable(ref v) => Some((v.clone(), request_segment)),
                        })
            .collect();
        Some((self,
              OutgoingHttpRequest {
                  method: request.method.clone(),
                  path: request.path.clone(),
                  pathParams: path_params,
                  body: request.body.clone(),
              }))
    }

    fn handle(&self, request: &OutgoingHttpRequest) -> io::Result<String> {
        let mut stream = UnixStream::connect(&self.process)?;
        write_json(&mut stream, &request)?;
        stream.shutdown(Shutdown::Write)?;

        let mut output = String::new();
        stream.read_to_string(&mut output)?;
        Ok(output)
    }
}

#[derive(Debug, Serialize)]
struct RoutePath(pub Vec<RoutePathSegment>);

#[derive(Debug, Serialize)]
enum RoutePathSegment {
    Fixed(String),
    Variable(String),
}

#[derive(Debug, Deserialize)]
struct IncomingHttpRequest {
    method: HttpMethod,
    path: String,
    body: String,
}

#[allow(non_snake_case)]
#[derive(Debug, Serialize)]
struct OutgoingHttpRequest {
    method: HttpMethod,
    path: String,
    pathParams: HashMap<String, String>,
    body: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HttpResponse {
    status: u16,
    body: String,
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
enum HttpMethod {
    GET,
    POST,
}

impl serde::Deserialize for HttpMethod {
    fn deserialize<D>(deserializer: D) -> Result<HttpMethod, D::Error>
        where D: serde::Deserializer
    {
        deserializer.deserialize_string(HttpMethodVisitor)
    }
}

struct HttpMethodVisitor;

impl serde::de::Visitor for HttpMethodVisitor {
    type Value = HttpMethod;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an HTTP method")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where E: serde::de::Error
    {
        self.visit_string(v.to_string())
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where E: serde::de::Error
    {
        let method = v.to_uppercase();
        match method.as_ref() {
            "GET" => Ok(HttpMethod::GET),
            "POST" => Ok(HttpMethod::POST),
            _ => Err(E::custom(format!("Invalid HTTP method: {}", method))),
        }
    }
}

impl serde::Deserialize for RoutePath {
    fn deserialize<D>(deserializer: D) -> Result<RoutePath, D::Error>
        where D: serde::Deserializer
    {
        deserializer.deserialize_string(RoutePathVisitor)
    }
}

struct RoutePathVisitor;

impl serde::de::Visitor for RoutePathVisitor {
    type Value = RoutePath;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an HTTP path")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where E: serde::de::Error
    {
        self.visit_string(v.to_string())
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where E: serde::de::Error
    {
        Ok(RoutePath(v.split("/")
                         .map(|segment| if segment.starts_with(":") {
                                  RoutePathSegment::Variable(segment[1..].to_string())
                              } else {
                                  RoutePathSegment::Fixed(segment.to_string())
                              })
                         .collect()))
    }
}
