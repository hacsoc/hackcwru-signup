//#![feature(convert)]

#[macro_use] extern crate nickel;
extern crate rustc_serialize;
extern crate hyper;
extern crate nickel_postgres;
extern crate postgres;
extern crate r2d2_postgres;
extern crate r2d2;

use nickel::{Nickel, HttpRouter, QueryString};
use nickel::extensions::Redirect;
use rustc_serialize::json;
use hyper::Client;
use hyper::header::Connection;
use std::io::Read;
use std::env;
use r2d2::{NopErrorHandler, PooledConnection};
use r2d2_postgres::{SslMode, PostgresConnectionManager};
use nickel_postgres::{PostgresMiddleware, PostgresRequestExtensions};

#[derive(RustcDecodable, RustcEncodable, Debug)]
struct Data {
    data: User,
}

#[derive(RustcDecodable, RustcEncodable, Debug)]
struct User {
    id: i32,
    email: String,
    created_at: String,
    updated_at: String,
    first_name: String,
    last_name: String,
    graduation: String,
    major: String,
    shirt_size: String,
    dietary_restrictions: String,
    special_needs: Option<String>,
    date_of_birth: String,
    gender: String,
    phone_number: String,
    school: School,
}

#[derive(RustcDecodable, RustcEncodable, Debug)]
struct School {
    id: i32,
    name: String,
}

#[derive(RustcDecodable, RustcEncodable, Debug)]
struct TokenResp {
    access_token: String,
    token_type: String,
    scope: String,
    created_at: u64,
}

#[derive(RustcDecodable, RustcEncodable, Debug)]
struct Payload {
    channel: String,
    username: String,
    text: String,
    icon_emoji: String,
}


macro_rules! optry {
    ( $x:expr ) => {
        match $x {
            Some(v) => v,
            None => return None,
        }
    }
}

fn do_request(code: &str) -> Option<Data> {
    let id = env::var("ID").expect("Failed to get ID value");
    let secret = env::var("SECRET").expect("Failed to get SECRET value");
    let redirect = env::var("REDIRECT").expect("Failed to get REDIRECT value");

    let url = format!("https://my.mlh.io/oauth/token?client_id={}&client_secret={}&code={}&redirect_uri={}&grant_type=authorization_code",
                      id, secret, code, redirect);

    let client = Client::new();
    let mut res = optry!(client.post(&url)
        .header(Connection::close())
        .send().ok());

    let mut body = String::new();
    optry!(res.read_to_string(&mut body).ok());

    let token: TokenResp = optry!(json::decode(&body).ok());
    let url2 = format!("https://my.mlh.io/api/v1/user?access_token={}",
                       token.access_token);

    res = optry!(client.get(&url2)
        .header(Connection::close())
        .send().ok());
    body = String::new();
    optry!(res.read_to_string(&mut body).ok());

    let person_data: Data = optry!(json::decode(&body).ok());

    let payload = Payload {
        channel: "#hackcwru".to_string(),
        username: "Signup bot".to_string(),
        icon_emoji: ":hackcwru:".to_string(),
        text: format!("{} from {} has signed up!",
                      person_data.data.first_name,
                      person_data.data.school.name),
    };

    let payload_str = json::encode(&payload).unwrap();

    let url3 = env::var("SLACKURL").expect("Failed to get slack url");

    let _res = optry!(client.post(&url3)
                 .body(&payload_str)
                 .send().ok());

    Some(person_data)
}

fn create_table(conn: PooledConnection<PostgresConnectionManager>) {
    let _r = conn.execute(
            "CREATE TABLE IF NOT EXISTS person (
                id SERIAL PRIMARY KEY,
                email VARCHAR NOT NULL,
                created_at VARCHAR NOT NULL,
                updated_at VARCHAR NOT NULL,
                first_name VARCHAR NOT NULL,
                last_name VARCHAR NOT NULL,
                graduation VARCHAR NOT NULL,
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
}

fn main() {
    let mut app = Nickel::new();

    let postgres_url = env::var("DATABASE")
        .expect("Failed to get DATABASE value");
    let dbpool = PostgresMiddleware::new(&*postgres_url, SslMode::None, 5,
                                         Box::new(NopErrorHandler))
        .expect("Failed to start PostgresMiddleware");

    create_table(dbpool.pool.clone().get().unwrap());
    app.utilize(dbpool);

    app.get("/callback", middleware! { |request, response|
        let conn = request.db_conn();
        let user_data = match request.query().get("code") {
            Some(s) => do_request(s),
            None => None
        }.unwrap().data;

        let r = conn.execute(
                "INSERT INTO person (id, email, created_at, updated_at,
                first_name, last_name, graduation, major, shirt_size,
                dietary_restrictions, special_needs, date_of_birth, gender,
                phone_number, school_id, school_name) VALUES ($1, $2, $3, $4,
                $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
                &[&user_data.id, &user_data.email, &user_data.created_at,
                  &user_data.updated_at, &user_data.first_name,
                  &user_data.last_name, &user_data.graduation, &user_data.major,
                  &user_data.shirt_size, &user_data.dietary_restrictions,
                  &user_data.special_needs, &user_data.date_of_birth,
                  &user_data.gender, &user_data.phone_number,
                  &user_data.school.id, &user_data.school.name]
            );
        match r {
            Ok(v) => println!("{:?}", v),
            Err(e) => println!("{:?}", e)
        }

        return response.redirect("http://hack.cwru.edu/register.html")
    });

    app.get("/start", middleware! { |_req, response|
        let id = env::var("ID").expect("Failed to get ID value");
        let redirect = env::var("REDIRECT")
            .expect("Failed to get REDIRECT value");
        return response.redirect(
            format!(
                    "http://my.mlh.io/oauth/authorize?client_id={}&redirect_uri={}&response_type=code",
                    id, redirect
                )
            );
    });

    let bind = match env::var("BIND") {
        Ok(v) => v,
        Err(_) => "127.0.0.1:8080".to_string(),
    };

    app.listen(&bind[..]);
}
