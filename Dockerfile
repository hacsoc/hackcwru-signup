FROM schickling/rust
MAINTAINER Matthew Bentley "bentley@case.edu"

ENV USER "Matthew Bentley"

ADD . /source/
WORKDIR /source/

RUN cargo build --release

CMD ["/source/target/release/hackcwru"]
