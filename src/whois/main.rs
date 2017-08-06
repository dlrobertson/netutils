#![feature(ascii_ctype)]

use std::process::exit;
use std::env::{args, Args};
use std::iter::Skip;
use std::error::Error;
use std::net::TcpStream;
use std::io::{Write, BufRead, BufReader};
use std::ascii::AsciiExt;

/* Some portion of this code is dedicated to parsing the arguments. This can easily be improved
 * if we can use ArgParser, but ArgParser has to be moved first to redox/libextra. See
 * https://github.com/redox-os/redox/issues/923 */

/* Print an error message and exit. Panicking instead won't print a nice output. This isn't a nice
 * solution and would be solved by using the feature in RFC 1937. See
 * https://github.com/rust-lang/rust/issues/43301 */
fn fatal_error(msg: String) {
    eprintln!("{}", msg);
    exit(1);
}

macro_rules! fatal_error {
    ($($arg:expr),*) => {fatal_error(format!($($arg),*))}
}

/* Store the next argument in var and fatally exit if there isn't any. If we can use ArgParser, this
 * needs to be removed. */
fn next_required_arg<T>(args: &mut Skip<Args>, option_str: &str, func: T)
where
    T: FnOnce(String),
{
    match args.next() {
        Some(s) => func(s),
        None => fatal_error!("option '{}' requires an argument", option_str),
    }
}

fn main() {
    // Set defaults
    let mut host = "whois.iana.org".to_string();
    let mut port: u16 = 43;
    let query: String;

    // Parse the arguments. This needs to change if we can use ArgParser.
    {
        let mut query_vec = Vec::with_capacity(1);
        let mut args = args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str(){
                "--help" => {
                    println!("Usage: whois [-h hostname] [-p port] query");
                    exit(0);
                }
                "-h" => // For easier case insenstive comparisons, lowercase the host.
                    next_required_arg(&mut args, "-h", |s| host = s.to_ascii_lowercase()),
                "-p" =>
                    next_required_arg(&mut args, "-p", |s| match s.parse::<u16>(){
                        Ok(num) => port = num,
                        Err(e) => fatal_error!("failed to parse '{}', {}", s, e.description())
                    }),
                _ => query_vec.push(arg)
            }
        }
        query = query_vec.join(" ");
    }

    // Remember previous hosts to prevent an infinte loop
    let mut previous_hosts = Vec::with_capacity(1);
    while host != "" {
        let mut nhost = "".to_string();
        // Connect to the whois host
        let connect_result = TcpStream::connect((host.as_str(), port));
        match connect_result {
            Ok(mut stream) => {
                // Send the query. A curfeed and a newline are required by the WHOIS standard.
                if let Err(e) = write!(stream, "{}\r\n", query) {
                    fatal_error!("Error sending to {}, {}", host, e.description());
                }

                /* Read the response and determine if it's a thick or a thin client. Unfortunately,
                 * there's no reliable way to differentiate between the two. The following method is
                 * borrowed from the FreeBSD whois client. */
                let mut reader = BufReader::new(stream);
                let mut line = String::with_capacity(64);
                'line_reading: loop {
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            print!("{}", line);
                            let trimmed_line = line.trim_left();
                            for prefix in [
                                "whois:",
                                "Whois Server:",
                                "Registrar WHOIS Server:",
                                "ReferralServer:  whois://",
                                "descr:          region. Please query",
                            ].iter()
                            {
                                if trimmed_line.starts_with(prefix) {
                                    if let Some(trimmed_line) = trimmed_line.get(prefix.len()..) {

                                        nhost = trimmed_line
                                            .trim_left()
                                            .trim_right_matches(|c: char| {
                                                !(c.is_ascii_alphanumeric() || c == '.' || c == '-')
                                            })
                                            .to_ascii_lowercase();

                                        //Print the rest of the whois data
                                        if let Err(e) = std::io::copy(
                                            &mut reader,
                                            &mut std::io::stdout(),
                                        )
                                        {
                                            fatal_error!(
                                                "Error printing whois data from {}, {}",
                                                host,
                                                e.description()
                                            );
                                        }
                                        break 'line_reading;
                                    }
                                    break;
                                }
                            }
                        }
                        Err(e) => fatal_error!("Error reading from {}, {}", host, e.description()),
                    }
                    line.clear();
                }
            }
            Err(e) => fatal_error!("Failed to connect to {}, {}", host, e.description()),
        }

        // Ignore and don't report an error for self-referrals
        if host == nhost {
            break;
        }

        // Check for and prevent referral loops
        {
            let mut previous_hosts_iter = previous_hosts.iter();
            if let Some(_) = previous_hosts_iter.position(|s| *s == nhost) {
                fatal_error!(
                    "Error: Detected whois referral loop between hosts:\n{}\n{}",
                    nhost,
                    previous_hosts_iter.as_slice().join("\n")
                );
            }
        }

        previous_hosts.push(host.clone());
        host = nhost;
    }
}
