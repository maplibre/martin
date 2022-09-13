FROM rust:alpine as builder

RUN apk update
RUN apk add --no-cache openssl-dev musl-dev

WORKDIR /usr/src/martin
ADD . .
RUN cargo build --release


FROM alpine:latest

COPY --from=builder \
  /usr/src/martin/target/release/martin \
  /usr/local/bin/

EXPOSE 3000
CMD /usr/local/bin/martin
