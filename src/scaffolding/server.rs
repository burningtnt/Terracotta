use crate::scaffolding::{PacketResponse, TIMEOUT};
use socket2::{Domain, SockAddr, Socket, Type};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::{io, thread};

pub type HandleFunction = fn(&[u8], Vec<u8>) -> io::Result<PacketResponse>;
pub type Handlers = &'static [(&'static str, &'static str, HandleFunction)];

pub fn start(handlers: Handlers, port: u16) -> io::Result<u16> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
    socket.bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)))?;
    socket.set_read_timeout(Some(TIMEOUT)).unwrap();
    socket.set_write_timeout(Some(TIMEOUT)).unwrap();
    socket.listen(128)?;

    let port = socket.local_addr().unwrap().as_socket().unwrap().port();

    thread::spawn(move || {
        let listener: TcpListener = socket.into();
        for mut stream in listener.incoming().flatten() {
            thread::spawn(move || {
                loop {
                    if let Err(e) = handle_connection(&mut stream, handlers) {
                        logging!("ScaffoldingServer", "Connection closed: {:?}", e);
                        return;
                    }
                }
            });
        }
    });

    Ok(port)
}

fn handle_connection(stream: &mut TcpStream, handlers: Handlers) -> io::Result<()> {
    let mut kind_size = [0u8; 1];
    stream.read_exact(&mut kind_size)?;
    let kind_size = kind_size[0] as usize;

    let mut kind = vec![0u8; kind_size];
    stream.read_exact(&mut kind)?;
    let kind = String::from_utf8(kind).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let kinds = kind.splitn(3, ':').collect::<Vec<_>>();
    if kinds.len() != 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid request kind."));
    }

    let mut body_size = [0u8; 4];
    stream.read_exact(&mut body_size)?;
    let body_size = u32::from_be_bytes(body_size) as usize;

    let mut body = vec![0u8; body_size];
    stream.read_exact(&mut body)?;

    static DEFAULT_HANDLE: HandleFunction = |_: &[u8], mut response: Vec<u8>| -> io::Result<PacketResponse> {
        response.extend_from_slice("Requested protocol hasn't been implemented.".as_bytes());
        PacketResponse::fail(255, response)
    };
    let handle = handlers.iter()
        .find(|(namespace, path, _)| kinds[0] == *namespace && kinds[1] == *path)
        .map(|(_, _, handle)| handle)
        .unwrap_or(&DEFAULT_HANDLE);

    let mut response = Vec::with_capacity(64);
    response.resize(5, 0u8);

    let (code, mut response) = match handle(&body, response) {
        Ok(PacketResponse::Ok { data }) => (0, data),
        Ok(PacketResponse::Fail { status, data}) => (status, data),
        Err(e) => {
            let mut response = Vec::with_capacity(64);
            response.resize(5, 0u8);
            if write!(&mut response, "{:?}", e).is_err() {
                response.truncate(5);
                response.extend_from_slice("Exception occurred when printing error message.".as_bytes())
            }
            (255, response)
        }
    };

    response[0] = code;
    let response_size = (response.len() - 5) as u32;
    response[1..5].copy_from_slice(&response_size.to_be_bytes());

    stream.write_all(&response)?;
    stream.flush()?;
    Ok(())
}