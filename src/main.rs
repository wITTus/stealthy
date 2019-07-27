mod logo;
mod tools;
mod rsatools;
mod arguments;
mod console;
mod view;
mod model;
mod keyboad;

use std::thread;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crypto::sha1::Sha1;
use crypto::digest::Digest;

use stealthy::{Message, IncomingMessage, Layers, Layer};
use crate::tools::{read_file, insert_delimiter, read_bin_file, write_data, decode_uptime, without_dirs};
use stealthy::xip::IpAddresses;

use crate::arguments::{parse_arguments, Arguments};
use crate::console::ConsoleMessage;

use crate::view::View;
use crate::keyboad::{InputKeyboard, UserInput};
use crate::model::{ItemType, Model, Item};
use std::iter::repeat;
use crate::model::Source;

type ArcModel = Arc<Mutex<Model>>;
type ArcView = Arc<Mutex<View>>;
type ConsoleSender = Sender<ConsoleMessage>;

fn help_message(o: ConsoleSender) {

    write_lines(o, &vec![
        "Commands always start with a slash:",
        " ",
        "/help              - this help message",
        "/uptime, /up       - uptime",
        "/cat <filename>    - send content of an UTF-8 encoded text file",
        "/upload <filename> - send binary file",
        " ",
        "Keys:",
        " ",
        "arrow up     - scroll to older messages",
        "arrow dow    - scroll to latest messages",
        "page up      - scroll one page up",
        "page down    - scroll one page down",
        "end          - scroll to last message in buffer",
        "ctrl+r       - switch to plain messages and back to normal view",
        "esc | ctrl+d - quit",
        " "
    ], ItemType::Info, Source::System);
}

fn write_lines(o: ConsoleSender, lines: &[&str], typ: ItemType, from: Source) {

    for v in lines {
        console::raw(o.clone(), String::from(*v), typ.clone(), from.clone())
    }
}



fn recv_loop(o: ConsoleSender, rx: Receiver<IncomingMessage>) {

    thread::spawn(move || {
        loop { match rx.recv() {
            Ok(msg) => process_incoming_message(o.clone(), msg),
            Err(e) => console::error(o.clone(), format!("recv_loop: failed to receive message. {:?}", e))
        }}
    });
}

fn process_incoming_message(o: ConsoleSender, msg: IncomingMessage) {

    match msg {
        IncomingMessage::New(msg) => { console::new_msg(o.clone(), msg); }
        IncomingMessage::Ack(id) => { console::ack_msg(o.clone(), id); }
        IncomingMessage::Error(_, s) => { console::error(o.clone(), s); }
        IncomingMessage::FileUpload(msg) => { process_upload(o.clone(), msg) }
        IncomingMessage::AckProgress(id, done, total) => { console::ack_msg_progress(o.clone(), id, done, total); }
    }
}

fn process_upload(o: ConsoleSender, msg: Message) {

    if msg.get_filename().is_none() {
        console::error(o.clone(), format!("Could not get filename of received file upload."));
        return;
    } else if msg.get_filedata().is_none() {
        console::error(o.clone(), format!("Could not get data of received file upload."));
        return;
    }

    let fname = msg.get_filename().unwrap();
    let data = msg.get_filedata().unwrap();
    let dst = format!("/tmp/stealthy_{}_{}", tools::random_str(10), &fname);
    console::new_file(o.clone(), msg, fname);

    if write_data(&dst, data) {
        console::status(o.clone(), format!("File written to '{}'.", dst));
    } else {
        console::error(o.clone(), format!("Could not write data of received file upload."));
    }
}


#[derive(Clone, Debug)]
pub struct GlobalState {
    start_time: time::Timespec
}

static mut GLOBAL_STATE: Option<GlobalState> = None;

// returns the uptime of stealthy in seconds
fn uptime() -> i64 {
    // TODO access to global state needs to be synchronized
    unsafe {
        time::get_time().sec - GLOBAL_STATE.clone().unwrap().start_time.sec
    }
}

fn init_global_state() {
    unsafe {
        GLOBAL_STATE = Some(GlobalState {
            start_time: time::get_time(),
        })
    };
}

fn parse_command(txt: String, o: ConsoleSender, l: &Layers, dstips: &IpAddresses) {
    // TODO: find more elegant solution for this
    if txt.starts_with("/cat ") {
        // TODO split_at works on bytes not characters
        let (_, b) = txt.as_str().split_at(5);
        match read_file(b) {
            Ok(data) => {
                console::msg(o.clone(), String::from("Transmitting data ..."), ItemType::Info, Source::System);
                let s = data.as_str();
                for line in s.split("\n") {
                    send_message(line.to_string().trim_end().to_string(), o.clone(), l, dstips);
                }
            },
            _ => {
                console::msg(o.clone(), String::from("Could not read file."), ItemType::Error, Source::System);
            }
        }
        return;
    }

    if txt.starts_with("/upload ") {
        let (_, b) = txt.as_str().split_at(8);
        match read_bin_file(b) {
            Ok(data) => {
                send_file(data, b.to_string(), o, l, dstips);
            },
            Err(s) => {
                console::msg(o, String::from(s), ItemType::Error, Source::System);
            }
        }
        return;
    }

    match txt.as_str() {
        "/help" => {
            help_message(o.clone());
        },
        "/uptime" | "/up" => {
            console::msg(o, format!("up {}", decode_uptime(uptime())), ItemType::Info, Source::System);
        },
        _ => {
            console::msg(o, String::from("Unknown command. Type /help to see a list of commands."), ItemType::Info, Source::System);
        }
    };
}

fn create_upload_data(dstip: String, fname: &String, data: &Vec<u8>) -> (Message, u64) {
    (
        Message::file_upload(dstip, without_dirs(fname), data),
        rand::random::<u64>()
    )
}

/// Sends a file in background.
///
/// # Arguments
///
/// * `data` - Content of the file (binary data).
/// * `fname` - Name of the file.
/// * `o` - Sender object to which messages are sent to.
fn send_file(data: Vec<u8>, fname: String, console: ConsoleSender, l: &Layers, dstips: &IpAddresses) {

    let n = data.len();

    // This is sent to the console to show the user information about the file upload.
    let mut item = Item::new(
        format!("sending file '{}' with {} bytes...", fname, n),
        ItemType::UploadMessage,
        model::Source::You
    ).add_size(n);

    // Create a tuple (Message, u64) for each destination IP. For each IP a unique ID is created.
    let v = dstips.as_strings()
        .iter()
        .map(|dstip| create_upload_data(dstip.clone(), &fname, &data))
        .collect::<Vec<_>>();

    // Add the file upload id to the item which is shown to the user. This ID allows us to
    // update the status of this item, e.g. once the file upload is finished.
    for (_, id) in &v {
        item = item.add_id(*id);
    }

    // Show the message.
    console::msg_item(console.clone(),item);

    // Now, start the file transfer in the background for each given IP.
    for (msg, id) in v {
        l.send(msg, id, true);
    }
}

fn create_data(dstip: String, txt: &String) -> (Message, u64) {
    (Message::new(dstip, txt.clone().into_bytes()), rand::random::<u64>())
}

fn send_message(txt: String, o: ConsoleSender, l: &Layers, dstips: &IpAddresses) {

    let mut item = Item::new(format!("{}", txt), ItemType::MyMessage, model::Source::You);

    let v = dstips.as_strings()
        .iter()
        .map(|dstip| create_data(dstip.clone(), &txt))
        .collect::<Vec<_>>();

    for (_, id) in &v {
        item = item.add_id(*id);
    }
    console::msg_item(o.clone(),item);

    for (msg, id) in v {
        l.send(msg, id, false);
    }
}

fn get_layer(args: &Arguments, status_tx: Sender<String>, dstips: &IpAddresses) -> Layer {
    let ret =
        if args.hybrid_mode {
            // use asymmetric encryption
            Layers::asymmetric(&args.rcpt_pubkey_file, &args.privkey_file, &args.device, status_tx, dstips)
        } else {
            // use symmetric encryption
            Layers::symmetric(&args.secret_key, &args.device, status_tx, dstips)
        };
    ret.expect("Initialization failed.")
}

fn chars(n: usize, c: char) -> String {
    repeat(c).take(n).collect()
}

fn normalize(v: &[&String], c: char) -> (Vec<String>, usize) {
    let maxlen = v.iter().map(|x| x.len()).max().unwrap();
    let r = v.iter()
        .map(|&s| s.clone() + &chars(maxlen - s.len() + 1, c))
        .collect::<Vec<String>>();
    let x = r.iter().map(|s| s.len()).max().unwrap();
    (r, x)
}

fn welcome(args: &Arguments, o: ConsoleSender, layer: &Layer, dstips: &IpAddresses) {
    for l in logo::get_logo() {
        console::raw(o.clone(), l, ItemType::Introduction, Source::System);
    }

    let ips = dstips.as_strings().join(", ");

    let (values, n) = normalize(&[&args.device, &ips, &ips], ' ');

    let v = vec![
        format!("The most secure ICMP messenger."),
        format!(" "),
        format!("┌─────────────────────┬─{}┐", chars(n, '─')),
        format!("│ Listening on device │ {}│", values[0]),
        format!("│ Talking to IPs      │ {}│", values[1]),
        format!("│ Accepting IPs       │ {}│", values[2]),
        format!("└─────────────────────┴─{}┘", chars(n, '─')),
        format!(" "),
        format!("Type /help to get a list of available commands."),
        format!("Check https://github.com/daniel-e/stealthy for more documentation."),
        format!("Esc or Ctrl+D to quit.")
    ];

    write_lines(
        o.clone(),
        v.iter().map(|x| x.as_str()).collect::<Vec<_>>().as_slice(),
        ItemType::Introduction,
            Source::System
    );

    if args.hybrid_mode {
        let mut h = Sha1::new();

        h.input(&layer.layers.encryption_key());
        let s = insert_delimiter(&h.result_str());
        console::raw(o.clone(), format!("Hash of encryption key : {}", s), ItemType::Introduction, Source::System);

        h.reset();
        h.input(&rsatools::key_as_der(&read_file(&args.pubkey_file).unwrap()));
        let q = insert_delimiter(&h.result_str());
        console::raw(o.clone(), format!("Hash of your public key: {}", q), ItemType::Introduction, Source::System);
    }
    console::raw(o.clone(), format!(" "), ItemType::Introduction, Source::System);
    console::raw(o.clone(), format!("Happy chatting..."), ItemType::Introduction, Source::System);
    console::raw(o.clone(), format!(" "), ItemType::Introduction, Source::System);
}







fn status_message_loop(o: ConsoleSender) -> Sender<String> {

    let (tx, rx) = channel::<String>();
    thread::spawn(move || { loop { match rx.recv() {
        Ok(msg) => console::status(o.clone(), msg),
        Err(er) => console::error(o.clone(), format!("status_message_loop: failed. {:?}", er))
    }}});
    tx
}

fn keyboad_loop(o: ConsoleSender, l: Layers, dstips: IpAddresses, model: ArcModel, view: ArcView) {

    let mut input = InputKeyboard::new();

    loop { match input.read_char() {
        UserInput::Character(buf) => {
            let mut v = vec![];
            for c in buf {
                let mut m = model.lock().unwrap();
                if c == 13 {
                    let s = m.apply_enter();
                    send_message(s, o.clone(), &l, &dstips);
                } else {
                    v.push(c);
                    if String::from_utf8(v.clone()).is_ok() {
                        m.update_input(v.clone());
                        v.clear();
                    }
                }
            }
            view.lock().unwrap().refresh();

        },
        UserInput::Escape | UserInput::CtrlD => {
            view.lock().unwrap().close();
            o.send(ConsoleMessage::Exit).expect("Send failed.");
            break;
        },
        UserInput::ArrowDown => {
            view.lock().unwrap().scroll_down();
        },
        UserInput::ArrowUp => {
            view.lock().unwrap().scroll_up();
        },
        UserInput::Backspace => {
            model.lock().unwrap().apply_backspace();
            view.lock().unwrap().refresh();
        },
        UserInput::End => {
            view.lock().unwrap().key_end();
        },
        UserInput::PageDown => {
            view.lock().unwrap().page_down();
        },
        UserInput::PageUp => {
            view.lock().unwrap().page_up();
        },
        UserInput::CtrlR => {
            view.lock().unwrap().toggle_raw_view();
        },
        UserInput::CtrlS => {
            view.lock().unwrap().toggle_scramble_view();
        },
        UserInput::Enter => {
            let s = model.lock().unwrap().apply_enter();
            view.lock().unwrap().refresh();
            if s.len() > 0 {
                if s.starts_with("/") {
                    parse_command(s, o.clone(), &l, &dstips);
                } else {
                    send_message(s, o.clone(), &l, &dstips);
                }
            }
        }
    }}
}

fn create_console_sender(model: ArcModel, view: ArcView) -> ConsoleSender {

    // The sender "tx" is used at other locations to send messages to the output.
    let (tx, rx) = channel::<ConsoleMessage>();

    thread::spawn(move || {
        loop { match rx.recv().unwrap() {
            ConsoleMessage::TextMessage(item) => {
                model.lock().unwrap().add_message(item.clone());
                view.lock().unwrap().adjust_scroll_offset(item);
            },
            ConsoleMessage::Ack(id) => {
                model.lock().unwrap().ack(id);
                view.lock().unwrap().refresh();
            },
            ConsoleMessage::AckProgress(id, done, total) => {
                model.lock().unwrap().ack_progress(id, done, total);
                view.lock().unwrap().refresh();
            },
            // We need this as otherwise "out" is not dropped and the terminal state
            // is not restored.
            ConsoleMessage::Exit => {
                break;
            }
        }}
    });
    tx
}

fn main() {
    init_global_state();

    // Parse command line arguments.
	let args = parse_arguments().expect("Cannot parse arguments");;

    let dstips = IpAddresses::from_comma_list(&args.dstip);

    // The model stores all information which is required to show the screen.
    let model = Arc::new(Mutex::new(Model::new()));

    let view = Arc::new(Mutex::new(View::new(model.clone())));

    let tx = create_console_sender(model.clone(), view.clone());

    // TODO replace status_message_loop by tx?
    // TODO have only one loop? for keyboard events, status message events and other events
    
    let layer = get_layer(&args, status_message_loop(tx.clone()), &dstips);

    welcome(&args, tx.clone(), &layer, &dstips);

    // this is the loop which handles messages received via rx
    recv_loop(tx.clone(), layer.rx);

    // Waits for data from the keyboard.
    // If data is received the model and the view will be updated.
    keyboad_loop(tx.clone(), layer.layers, dstips, model, view);
}

