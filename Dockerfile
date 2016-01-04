FROM schickling/rust
MAINTAINER Matthew Bentley "bentley@case.edu"

ENV USER "Matthew Bentley"

RUN mkdir /hackcwru/
ADD . /hackcwru/
WORKDIR /hackcwru/

RUN cargo build --release

CMD ["/hackcwru/target/release/hackcwru"]
