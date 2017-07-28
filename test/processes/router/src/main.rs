extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate slog_json;

use slog::Drain;
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
    let request: HttpRequest = parse_json(&input.trim().to_string())?;
    let route = routes.iter().find(|route| route.matches(&request));
    let output = match route {
        Some(route) => route.handle(&request),
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
    path: String,
    process: String,
}

impl Route {
    fn matches(&self, request: &HttpRequest) -> bool {
        self.method == request.method && self.path == request.path
    }

    fn handle(&self, request: &HttpRequest) -> io::Result<String> {
        let mut stream = UnixStream::connect(&self.process)?;
        write_json(&mut stream, &request)?;
        stream.shutdown(Shutdown::Write)?;

        let mut output = String::new();
        stream.read_to_string(&mut output)?;
        Ok(output)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct HttpRequest {
    method: HttpMethod,
    path: String,
    body: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HttpResponse {
    status: u16,
    body: String,
}

#[derive(PartialEq, Eq, Debug, Serialize)]
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
