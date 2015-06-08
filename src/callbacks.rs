extern crate time;

use term::color;
use std::process::Command;

use humaninterface::Output;
use icmpmessaging::Message;

pub trait Callbacks : Output {

    /// This function is called when a new message has been received.
    fn new_msg(&mut self, msg: Message) {

        let ip = msg.get_ip();
        let s  = String::from_utf8(msg.get_payload());
        let fm = time::strftime("%R", &time::now()).unwrap();

        match s {
            Ok(s)  => { 
                self.println(format!("{} [{}] says: {}", fm, ip, s), color::YELLOW);

                let output = Command::new("notify-send")
                    .arg("-t")
                    .arg("3000")
                    .arg(format!("new message from {}", ip)).output();
            }
            Err(_) => { 
                self.println(format!("[{}] {} error: could not decode message", ip, fm), color::BRIGHT_RED); 
            }
        }
    }

    /// This callback function is called when the receiver has received the
    /// message with the given id.
    ///
    /// Important note: The acknowledge that is received here is the ack on the
    /// network layer which is not protected. An
    /// attacker could drop acknowledges or could fake acknowledges. Therefore,
    /// it is important that acknowledges are handled on a higher layer where
    /// they can be protected via cryptographic mechanisms.
    fn ack_msg(&mut self, _id: u64) {

        self.println("ack".to_string(), color::BRIGHT_GREEN);
    }
}


