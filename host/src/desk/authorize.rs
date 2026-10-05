use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;

use jsrt::rquickjs::{self, Ctx, Exception, Function, Value, function::{Async, Opt}};

const DEFAULT_PORT: u16 = 8888;

pub fn authorize<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(
        ctx.clone(),
        Async(|ctx: Ctx<'js>, url: String, port: Opt<u16>| async move {
            let port = port.0.unwrap_or(DEFAULT_PORT);

            if url.contains("{{") {
                return Err(Exception::throw_message(
                    &ctx,
                    &format!("authorize url still has an unsubstituted placeholder: {url}"),
                ));
            }

            let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
                Exception::throw_message(&ctx, &format!("binding 127.0.0.1:{port}: {e}"))
            })?;

            println!("\nopen this to authorize:\n\n  {url}\n");
            let _ = Command::new("xdg-open").arg(&url).status();

            wait_for_code(&listener).map_err(|e| Exception::throw_message(&ctx, &e))
        }),
    )?;

    Ok(function.into_value())
}

fn wait_for_code(listener: &TcpListener) -> Result<String, String> {
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|e| format!("accepting the redirect: {e}"))?;

        let mut line = String::new();
        BufReader::new(&stream)
            .read_line(&mut line)
            .map_err(|e| format!("reading the redirect: {e}"))?;

        let target = line.split_whitespace().nth(1).unwrap_or_default();
        let code = query(target, "code");
        let error = query(target, "error");

        let body = match (&code, &error) {
            (Some(_), _) => "<h1>Authorized</h1><p>You can close this tab.</p>",
            (None, Some(_)) => "<h1>Denied</h1><p>Authorization was refused.</p>",
            _ => "<h1>Waiting</h1><p>Nothing to do here yet.</p>",
        };
        let _ = respond(&mut stream, body);

        if let Some(code) = code {
            return Ok(code);
        }

        if let Some(error) = error {
            return Err(format!("the provider refused: {error}"));
        }
    }

    Err("the listener closed before a redirect arrived".to_string())
}

fn respond(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

fn query(target: &str, key: &str) -> Option<String> {
    let (_, query) = target.split_once('?')?;

    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn loopback() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("binding an ephemeral port");
        let addr = listener.local_addr().expect("reading the bound port");
        (listener, addr)
    }

    fn request(addr: SocketAddr, target: &str) {
        let mut stream = TcpStream::connect(addr).expect("connecting to the listener");
        write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("writing the request line");
    }

    #[test]
    fn reads_the_code_out_of_a_redirect() {
        let (listener, addr) = loopback();
        std::thread::spawn(move || request(addr, "/callback?code=abc123&state=xyz"));

        assert_eq!(wait_for_code(&listener).unwrap(), "abc123");
    }

    #[test]
    fn a_favicon_request_does_not_end_the_wait() {
        let (listener, addr) = loopback();
        std::thread::spawn(move || {
            request(addr, "/favicon.ico");
            request(addr, "/callback?code=second");
        });

        assert_eq!(wait_for_code(&listener).unwrap(), "second");
    }

    #[test]
    fn a_refusal_is_an_error_rather_than_a_hang() {
        let (listener, addr) = loopback();
        std::thread::spawn(move || request(addr, "/callback?error=access_denied"));

        let err = wait_for_code(&listener).unwrap_err();
        assert!(err.contains("access_denied"), "{err}");
    }

    #[test]
    fn the_browser_gets_a_well_formed_response() {
        let (listener, addr) = loopback();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connecting");
            write!(stream, "GET /cb?code=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();

            let mut reply = String::new();
            std::io::Read::read_to_string(&mut stream, &mut reply).expect("reading the reply");
            reply
        });

        wait_for_code(&listener).unwrap();
        let reply = client.join().expect("the client thread");

        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");

        let (head, body) = reply.split_once("\r\n\r\n").expect("a blank line before the body");
        let declared: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .expect("a Content-Length")
            .trim()
            .parse()
            .expect("a numeric Content-Length");

        assert_eq!(declared, body.len(), "Content-Length does not match the body");
    }

    #[test]
    fn the_code_is_found_whatever_the_parameter_order() {
        assert_eq!(query("/cb?state=xyz&code=abc", "code").unwrap(), "abc");
        assert_eq!(query("/cb?code=abc", "code").unwrap(), "abc");
    }

    #[test]
    fn a_target_with_no_code_yields_nothing() {
        assert!(query("/favicon.ico", "code").is_none());
        assert!(query("/cb?", "code").is_none());
        assert!(query("/cb?state=xyz", "code").is_none());
        assert!(query("/cb?codeish=abc", "code").is_none());
    }

    #[test]
    fn percent_escapes_are_decoded() {
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("one+two"), "one two");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
