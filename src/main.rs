//#![feature(convert)]

#[macro_use]
extern crate nickel;
extern crate rustc_serialize;
extern crate hyper;
extern crate nickel_postgres;
extern crate postgres;
extern crate r2d2_postgres;
extern crate r2d2;
extern crate time;

use nickel::{Nickel, HttpRouter, QueryString};
use nickel::extensions::Redirect;
use nickel::status::StatusCode;
use rustc_serialize::json;
use hyper::Client;
use hyper::header::Connection;
use hyper::status::StatusClass;
use hyper::client::response::Response;
use std::io::Read;
use std::env;
use r2d2::{NopErrorHandler, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};
use nickel_postgres::{PostgresMiddleware, PostgresRequestExtensions};
use time::now;

#[derive(RustcDecodable, Debug)]
struct Data {
    data: User,
}

#[derive(RustcDecodable, Debug)]
struct User {
    id: i32,
    email: String,
    created_at: String,
    updated_at: String,
    first_name: String,
    last_name: String,
    major: String,
    shirt_size: String,
    dietary_restrictions: String,
    special_needs: Option<String>,
    date_of_birth: String,
    gender: String,
    phone_number: String,
    school: School,
}

#[derive(RustcDecodable, Debug)]
struct School {
    id: i32,
    name: String,
}

#[derive(RustcDecodable, Debug)]
struct TokenResp {
    access_token: String,
    token_type: String,
    scope: String,
    created_at: u64,
}

#[derive(RustcEncodable, Debug)]
struct Payload {
    channel: String,
    username: String,
    text: String,
    icon_emoji: String,
}

#[derive(Debug)]
enum ApiError {
    ClientError,
    ServerError,
}

#[derive(Debug)]
enum RequestError {
    Hyper(hyper::error::Error),
    Io(std::io::Error),
    JsonEnc(rustc_serialize::json::EncoderError),
    JsonDec(rustc_serialize::json::DecoderError),
    Api(ApiError),
}

// `env_err!(var, message)` gets `var` from the environment, and panics with
// `message` if that fails
// `env_err!(var)` gets `var` from the environment and panics with `"Failed to
// get env var $var"` if that fails
macro_rules! env_err {
    ( $var:expr, $error:expr ) => {
        env::var($var).expect($error)
    };
    ( $var:expr ) => {
        env_err!($var, &format!("Failed to get env var {}", $var))
    }
}

// Error handling code.  Expands to impl-ing From for enum_t from from_t to to_t
// See https://doc.rust-lang.org/book/error-handling.html for a full description
macro_rules! impl_from {
    ( $from_t:path, $to_t:path, $enum_t:path ) => {
        impl From<$from_t> for $enum_t {
            fn from(err: $from_t) -> $enum_t {
                $to_t(err)
            }
        }
    }
}

// All the impls for RequestError
impl_from!(hyper::error::Error, RequestError::Hyper, RequestError);
impl_from!(std::io::Error, RequestError::Io, RequestError);
impl_from!(rustc_serialize::json::DecoderError,
           RequestError::JsonDec,
           RequestError);
impl_from!(rustc_serialize::json::EncoderError,
           RequestError::JsonEnc,
           RequestError);
impl_from!(ApiError, RequestError::Api, RequestError);

// Checks for 4xx or 5xx errors and returns the appropriate ApiError
fn check_http_error(res: &Response) -> Result<(), ApiError> {
    match res.status.class() {
        StatusClass::ClientError => Err(ApiError::ClientError),
        StatusClass::ServerError => Err(ApiError::ServerError),
        _ => Ok(()),
    }
}

// Do the OAUTH stuff for my.mlh.io
// TODO: Something better about the long api urls
fn do_request(code: &str) -> Result<Data, RequestError> {
    let id = env_err!("ID");
    let secret = env_err!("SECRET");
    let redirect = env_err!("REDIRECT");

    let url = format!("https://my.mlh.io/oauth/token?client_id={}&\
                      client_secret={}&code={}&redirect_uri={}&\
                      grant_type=authorization_code",
                      id,
                      secret,
                      code,
                      redirect);

    let client = Client::new();
    let mut res = try!(client.post(&url)
        .header(Connection::close())
        .send());

    try!(check_http_error(&res));

    let mut body = String::new();
    try!(res.read_to_string(&mut body));

    let token: TokenResp = try!(json::decode(&body));
    let url2 = format!("https://my.mlh.io/api/v2/user?access_token={}",
                       token.access_token);

    res = try!(client.get(&url2)
        .header(Connection::close())
        .send());

    try!(check_http_error(&res));

    body = String::new();
    try!(res.read_to_string(&mut body));

    let person_data: Data = try!(json::decode(&body));

    Ok(person_data)
}

// Send a message to slack when a new user signs up
fn slack_send(user: User) -> Result<(), RequestError> {
    let url = env_err!("SLACKURL");
    let client = Client::new();
    let payload = Payload {
        channel: "#signups".to_string(),
        username: "Signup bot".to_string(),
        icon_emoji: ":hackcwru:".to_string(),
        text: format!("{}, a {} major from {}, has signed up!",
                      user.first_name,
                      user.major,
                      user.school.name),
    };
    let payload_str = try!(json::encode(&payload));
    let res = try!(client.post(&url)
        .body(&payload_str)
        .send());

    try!(check_http_error(&res));

    Ok(())
}

// Create the table if it doesn't exist.  Runs on each startup
fn create_table(conn: PooledConnection<PostgresConnectionManager>) {
    let _r =
        conn.execute("CREATE TABLE IF NOT EXISTS person (
                id SERIAL PRIMARY KEY,
                email VARCHAR NOT NULL,
                created_at VARCHAR NOT NULL,
                updated_at VARCHAR NOT NULL,
                first_name VARCHAR NOT NULL,
                last_name VARCHAR NOT NULL,
                graduation VARCHAR,
                major VARCHAR NOT NULL,
                shirt_size VARCHAR NOT NULL,
                dietary_restrictions VARCHAR NOT NULL,
                special_needs VARCHAR,
                date_of_birth VARCHAR NOT NULL,
                gender VARCHAR NOT NULL,
                phone_number VARCHAR NOT NULL,
                school_id integer,
                school_name VARCHAR
                )",
            &[]
        );
    let _r = conn.execute("CREATE TABLE IF NOT EXISTS email (email VARCHAR \
                          PRIMARY KEY)",
                          &[]);
    let _r = conn.execute("ALTER TABLE person ADD COLUMN year integer \
                          NOT NULL DEFAULT 2016",
                          &[]);
    let _r = conn.execute("ALTER TABLE person ALTER COLUMN graduation \
                          DROP NOT NULL",
                          &[]);
    let _r = conn.execute(
            "ALTER TABLE person ADD COLUMN signup_time VARCHAR", &[]
        );
    let _r = conn.execute(
            "ALTER TABLE person ADD COLUMN signed_in
            BOOLEAN NOT NULL DEFAULT FALSE",
        &[]
        );
    let _r = conn.execute(
            "ALTER TABLE person ADD COLUMN signed_in_time VARCHAR", &[]
        );

    let _r = conn.execute(
            "ALTER TABLE person DROP CONSTRAINT person_pkey", &[]
        );

    let _r = conn.execute(
            "ALTER TABLE person ADD PRIMARY KEY (id, year)", &[]
        );
}

fn main() {
    let mut app = Nickel::new();

    let postgres_url = env_err!("DATABASE");
    let dbpool =
        PostgresMiddleware::new(&*postgres_url, TlsMode::None, 5, Box::new(NopErrorHandler))
            .expect("Failed to start PostgresMiddleware");

    create_table(dbpool.pool.clone().get().unwrap());
    app.utilize(dbpool);

    app.get("/callback", middleware! { |request, mut response|
        let conn = request.db_conn();
        let redirect_success = env_err!("FINALREDIRECT");
        let redirect_failure = env_err!("REDIRECTERR");
        let year = env_err!("YEAR").parse::<i32>().unwrap();
        let code = match request.query().get("code") {
            Some(s) => s,
            None => {
                println!("No code, quitting");
                return response.error(StatusCode::BadRequest, "Failed");
            }
        };
        let user_data = match do_request(code) {
            Ok(s) => s,
            Err(e) => {
                println!("Get user data failed with error: {:?}", e);
                return response
                    .error(StatusCode::InternalServerError, "Failed");
            }
        }.data;

        let now = now();
        let now_s = format!("{}", now.rfc3339());
        // TODO: make this a function
        let r = conn.execute(
                "INSERT INTO person (id, email, created_at, updated_at,
                first_name, last_name, major, shirt_size,
                dietary_restrictions, special_needs, date_of_birth, gender,
                phone_number, school_id, school_name, year, signup_time) VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17)",
                &[&user_data.id, &user_data.email, &user_data.created_at,
                  &user_data.updated_at, &user_data.first_name,
                  &user_data.last_name, &user_data.major,
                  &user_data.shirt_size, &user_data.dietary_restrictions,
                  &user_data.special_needs, &user_data.date_of_birth,
                  &user_data.gender, &user_data.phone_number,
                  &user_data.school.id, &user_data.school.name, &year, &now_s]
            );
        let redirect = match r {
            Ok(v) => {
                println!("Add to database succeeded with status {:?}", v);
                match slack_send(user_data) {
                    Ok(_) => println!("Slack send worked"),
                    Err(e) => println!("Slack send failed with error: {:?}", e)
                }
                redirect_success
            },
            Err(e) => {
                println!("Add to database failed: {:?}", e);
                redirect_failure
            }
        };

        return response.redirect(&redirect[..]);
    });

    app.get("/start", middleware! { |_req, response|
        let id = env_err!("ID");
        let redirect = env_err!("REDIRECT");
        return response.redirect(
            format!(
                    "http://my.mlh.io/oauth/authorize?client_id={}&\
                    redirect_uri={}&response_type=code",
                    id, redirect
                )
            );
    });

    app.get("/email", middleware! { |req, res|
        let conn = req.db_conn();
        let email = req.query().get("email").unwrap();
        let redirect_success = env_err!("FINALREDIRECT");
        let redirect_failure = env_err!("REDIRECTERR");
        let r = conn.execute("INSERT INTO email VALEUS $1", &[&email]);
        let redirect = match r {
            Ok(v) => {
                println!("Add to database succeeded with status {:?}", v);
                redirect_success
            },
            Err(e) => {
                println!("Add to database failed: {:?}", e);
                redirect_failure
            }
        };

        return res.redirect(&redirect[..]);
    });

    let bind = match env::var("BIND") {
        Ok(v) => v,
        Err(_) => "127.0.0.1:8080".to_string(),
    };

    let _ = app.listen(&bind[..]);
}
