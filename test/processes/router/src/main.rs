extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate slog_json;
extern crate unix_socket;

use slog::Drain;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use unix_socket::{UnixListener, UnixStream};

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
        match handle(&configuration, &logger, &input) {
            Ok(response) => {
                connection.write_fmt(format_args!("{}", &response))?;
            }
            Err(error) => {
                error!(logger, "Connection error: {}", &error);
                connection.write("{}".as_bytes())?;
            }
        }
    }
    Ok(())
}

fn handle(configuration: &Configuration,
          logger: &slog::Logger,
          input: &String)
          -> io::Result<String> {
    let request: HttpRequest = parse_json(&input)?;
    let route = configuration
        .routes
        .iter()
        .find(|route| request.method == route.method && request.path == route.path);
    info!(logger, "request"; "request" => &input);
    let output = match route {
        Some(route) => {
            let mut stream = UnixStream::connect(&route.process)?;
            stream.write_fmt(format_args!("{}\n", &input))?;
            let mut output = String::new();
            stream.read_to_string(&mut output)?;
            output
        }
        None => NOT_FOUND.to_string(),
    };
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
    serde_json::from_str(&input).map_err(|error| io::Error::new(io::ErrorKind::Other, error))
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

#[derive(Serialize, Deserialize)]
struct HttpRequest {
    method: HttpMethod,
    path: String,
}

#[derive(Serialize, Deserialize)]
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
