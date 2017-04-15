extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate unix_socket;

use std::borrow::Cow;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::io::Write;
use std::path::Path;
use unix_socket::UnixStream;

macro_rules! print_err {
    ($($arg:tt)*) => (
        match write!(&mut ::std::io::stderr(), $($arg)* ).map(|_| io::stderr().flush().unwrap()) {
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

fn main() {
    let configuration = read_configuration();

    let not_found_response = HttpResponse {
        status: 404,
        body: "".to_string(),
    };
    let not_found = serde_json::to_string(&not_found_response).unwrap() + "\n";

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let input = line.unwrap();
        let request: HttpRequest = serde_json::from_str(&input).unwrap();
        let route = configuration
            .routes
            .iter()
            .find(|route| request.method == route.method && request.path == route.path);
        println_err!("Request: {:?} {}", request.method, &request.path);
        let response = match route {
            Some(route) => {
                let mut stream = UnixStream::connect(&route.process).unwrap();
                stream.write_fmt(format_args!("{}\n", input)).unwrap();
                let mut output = String::new();
                stream.read_to_string(&mut output).unwrap();
                print_err!("Response: {}", &output);
                Cow::Owned(output)
            }
            None => {
                print_err!("Response: {}", &not_found);
                Cow::Borrowed(&not_found)
            }
        };
        print!("{}", response);
        io::stdout().flush().unwrap();
    }
}

fn read_configuration() -> Configuration {
    let arg = env::args().nth(1).unwrap();
    let file_path = &Path::new(&arg);
    let mut file = File::open(file_path).unwrap();
    let mut string = String::new();
    file.read_to_string(&mut string).unwrap();
    serde_json::from_str(&string).unwrap()
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
