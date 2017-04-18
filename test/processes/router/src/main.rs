extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate unix_socket;

use std::env;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::io::Write;
use std::path::Path;
use unix_socket::{UnixListener, UnixStream};

macro_rules! print_err {
    ($($arg:tt)*) => (
        match write!(&mut ::std::io::stderr(), $($arg)* ).and_then(|_| io::stderr().flush()) {
            Ok(_) => {},
            Err(x) => panic!("Unable to write to stderr (file handle closed?): {}", x),
        }
    )
}

macro_rules! println_err {
    ($($arg:tt)*) => (
        match writeln!(&mut ::std::io::stderr(), $($arg)* ) {
            Ok(_) => {},
            Err(x) => panic!("Unable to write to stderr (file handle closed?): {}", x),
        }
    )
}

const NOT_FOUND: &'static str = "{\"status\":404,\"body\":\"\"}\n";


fn main() {
    let configuration = read_configuration().unwrap();

    let listener = UnixListener::bind(&Path::new(&env::args().nth(1).unwrap())).unwrap();
    for c in listener.incoming() {
        let mut connection = c.unwrap();
        let mut input = String::new();
        connection.read_to_string(&mut input).unwrap();
        match handle(&configuration, input) {
            Ok(response) => {
                connection
                    .write_fmt(format_args!("{}", response))
                    .unwrap();
            }
            Err(error) => {
                println_err!("{}", error);
                connection.write_fmt(format_args!("{{}}")).unwrap();
            }
        }
    }
}

fn handle(configuration: &Configuration, input: String) -> io::Result<String> {
    let request: HttpRequest = serde_json::from_str(&input)
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
    let route = configuration
        .routes
        .iter()
        .find(|route| request.method == route.method && request.path == route.path);
    print_err!("Request: {}", &input);
    match route {
        Some(route) => {
            let mut stream = UnixStream::connect(&route.process)?;
            stream.write_fmt(format_args!("{}\n", input))?;
            let mut output = String::new();
            stream.read_to_string(&mut output)?;
            print_err!("Response: {}", &output);
            Ok(output)
        }
        None => {
            print_err!("Response: {}", &NOT_FOUND);
            Ok(NOT_FOUND.to_string())
        }
    }
}

fn read_configuration() -> Result<Configuration, serde_json::Error> {
    let arg =
        env::args()
            .nth(2)
            .ok_or(io::Error::new(io::ErrorKind::Other, "No argument provided.".to_string()))?;
    let file_path = &Path::new(&arg);
    let mut file = File::open(file_path)?;
    let mut string = String::new();
    file.read_to_string(&mut string)?;
    serde_json::from_str(&string)
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
