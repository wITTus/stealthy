mod binding;
mod blowfish;
mod crypto;
mod delivery;
mod packet;
mod rsa;

use std::thread;
use std::sync::Arc;
use std::sync::mpsc::{channel, Receiver, Sender};

use crypto::{Encryption, SymmetricEncryption, AsymmetricEncryption};  // Implemenation for encryption layer
use delivery::Delivery;
use binding::Network;

pub enum ErrorType {
    DecryptionError,
    ReceiveError,
}

pub enum IncomingMessage {
    New(Message),
    Ack(u64),
    Error(ErrorType, String),
}

unsafe impl Sync for IncomingMessage { } // TODO XXX is it thread safe?
// http://doc.rust-lang.org/std/marker/trait.Sync.html

pub enum MessageType {
    NewMessage,
    AckMessage
}


impl Clone for MessageType {
    fn clone(&self) -> MessageType { 
        match *self {
            MessageType::NewMessage => MessageType::NewMessage,
            MessageType::AckMessage => MessageType::AckMessage
        }
    }
}


pub enum Errors {
	MessageTooBig,
	SendFailed,
    EncryptionError
}


pub struct Message {
    /// Contains the destination ip for outgoing messages, source ip from incoming messages.
	ip : String,
    typ: MessageType,
	buf: Vec<u8>,
}


impl Message {
	pub fn new(ip: String, buf: Vec<u8>) -> Message { Message::create(ip, buf, MessageType::NewMessage) }

	pub fn ack(ip: String) -> Message { Message::create(ip, vec![], MessageType::AckMessage) }

    pub fn set_payload(&self, buf: Vec<u8>) -> Message { 
        Message::create(self.get_ip(), buf, self.get_type())
    }

    pub fn get_payload(&self) -> Vec<u8> { self.buf.clone() }

    /// Returns the destination ip for outgoing messages or the source ip from incoming messages.
    pub fn get_ip(&self) -> String { self.ip.clone() }

    pub fn get_type(&self) -> MessageType { self.typ.clone() }

    fn create(ip: String, buf: Vec<u8>, typ: MessageType) -> Message {
		Message {
			ip: ip,
			buf: buf,
            typ: typ,
		}
	}
}

pub struct Layer {
    pub rx    : Receiver<IncomingMessage>,
    pub layers: Layers,
}


pub struct Layers {
    encryption_layer: Arc<Box<Encryption>>,
    delivery_layer  : Delivery
}


impl Layers {

    pub fn symmetric(hexkey: &String, device: &String) -> Result<Layer, &'static str> {

        Layers::init(Box::new(try!(SymmetricEncryption::new(hexkey))), device)
    }

    pub fn asymmetric(pubkey_file: &String, privkey_file: &String, device: &String) -> Result<Layer, &'static str> {

        match AsymmetricEncryption::new(&pubkey_file, &privkey_file) {
            Some(e) => Layers::init(Box::new(e), device),
            _ => Err("todo") // TODO
        }
    }

    pub fn send(&self, msg: Message) -> Result<u64, Errors> {

        match self.encryption_layer.encrypt(&msg.buf) {
            Ok(buf) => self.delivery_layer.send_msg(msg.set_payload(buf)),
            _ => Err(Errors::EncryptionError)
        }
    }

    // ------ private functions

    fn init(e: Box<Encryption>, device: &String) -> Result<Layer, &'static str> {

        // network  tx1 --- incoming message ---> rx1 delivery
        // delivery tx2 --- incoming message ---> rx2 layers
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        Ok(Layers::new(e,
            Delivery::new(Network::new(device, tx1), tx2, rx1),
            rx2
        ))
    }

    fn new(e: Box<Encryption>, d: Delivery, rx_network: Receiver<IncomingMessage>) -> Layer {

        // tx is used to send received messages to the application via rx
        let (tx, rx) = channel::<IncomingMessage>();

        let l = Layers {
            encryption_layer: Arc::new(e),
            delivery_layer: d,
        };

        l.recv_loop(tx, rx_network);
        Layer {
            rx: rx,
            layers: l
        }
    }

    /// Listens for incoming messages and processes them.
    fn recv_loop(&self, tx: Sender<IncomingMessage>, rx: Receiver<IncomingMessage>) {

        let enc = self.encryption_layer.clone();
        thread::spawn(move || { loop { match rx.recv() {
            Ok(msg) => match Layers::handle_message(msg, enc.clone()) {
                Some(m) => match tx.send(m) {
                    Err(_) => panic!("Channel closed."),
                    _ => { }
                },
                _ => Layers::err(ErrorType::DecryptionError, "Could not decrypt received message.", &tx)
            },
            _ => Layers::err(ErrorType::ReceiveError, "Could not receive message.", &tx)
        }}});
    }

    /// Notifies the application about an error.
    fn err(e: ErrorType, msg: &str, tx: &Sender<IncomingMessage>) {

        match tx.send(IncomingMessage::Error(e, msg.to_string())) {
            Ok(_) => { }
            // If the receiver has hung up quit the application.
            _ => panic!("Channel closed.")
        }
    }

    /// Decrypts incoming messages of type "new" or returns the message without
    /// modification if it is not of type "new".
    fn handle_message(m: IncomingMessage, enc: Arc<Box<Encryption>>) -> Option<IncomingMessage> {

        match m {
            IncomingMessage::New(msg) => {
                match enc.decrypt(&msg.buf) {
                    Ok(buf) => Some(IncomingMessage::New(msg.set_payload(buf))),
                    _ => None
                }
            }
            _ => Some(m)
        }
    }
}


// ------------------------------------------------------------------------
// TESTS
// ------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[test]
    fn test_handle_message() {


    }
}
