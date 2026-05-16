use dbus::arg::{Append, Arg, RefArg};
use dbus::blocking::Connection;

use std::collections::HashMap;
use std::time::Duration;

mod types;
use types::*;

macro_rules! test_method {
    ($proxy:ident $method_name:literal $out_struct:ty,$out_type:ty) => {
        let response_res: Result<$out_type, dbus::Error> =
            $proxy.method_call(NOTIFICATION_BUS_INTERFACE_NAME, $method_name, ());
        if let Ok(response) = response_res {
            let method_out = <$out_struct>::from(response);
            println!(
                "Server supports method '{}'. Output received:",
                $method_name
            );
            println!("{}", method_out);
        } else {
            println!(
                "Method '{}' is not supported by the notification server.",
                $method_name
            );
        }
    };
}

macro_rules! as_variant {
    ($value:literal) => {
        dbus::arg::Variant(Box::new($value) as Box<dyn dbus::arg::RefArg>)
    };
}

struct ApplicationOptions {
    basic: bool,
    basic_notification: bool,
    expire_timeout: bool,
    multiple_notifications: bool,
}

impl Default for ApplicationOptions {
    fn default() -> Self {
        ApplicationOptions {
            basic: true,
            basic_notification: false,
            expire_timeout: false,
            multiple_notifications: false,
        }
    }
}

impl ApplicationOptions {
    pub fn enable_all(&mut self) {
        self.basic = true;
        self.basic_notification = true;
        self.expire_timeout = true;
        self.multiple_notifications = true;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = ApplicationOptions::default();

    let arguments = std::env::args();
    let mut args_iter = arguments.into_iter();
    args_iter.next(); // Skip application name argument.

    while let Some(argument) = args_iter.next() {
        match argument.as_str() {
            "--all" => options.enable_all(),
            "--basic" => options.basic = true,
            "--no-basic" => options.basic = false,
            "--basic-notification" => options.basic_notification = true,
            "--expire-timeout" => options.expire_timeout = true,
            "--multiple-notifications" => options.multiple_notifications = true,
            "--help" => {
                println!("freedesktop-notification-tester");
                println!("");
                println!("usage: freedesktop-notification-tester [flags]");
                println!("");
                println!("Available flags are:");
                println!("{}", [
                    "  --all: Enable all available tests.",
                    "  --[no-]basic (default: on): Test availability of basic methods (server information and capabilities).",
                    "  --basic-notification: Test basic notification functionality (app_name, summary, body).",
                    "  --expire-timeout: Test support for the 'expire_timeout' notification argument.",
                    "  --multiple-notifications: Test support for multiple notifications at once.",
                ].join("\n"));
                println!("  --help:      Show this help menu.");
                println!("");
                println!("For more information, access the website:");
                println!("https://github.com/flowln/bell");

                return Ok(());
            }
            unrecognized => eprintln!("Unrecognized option '{}'", unrecognized),
        }
    }

    let conn = Connection::new_session()?;

    let mut proxy = conn.with_proxy(
        NOTIFICATION_BUS_INTERFACE_NAME,
        NOTIFICATION_BUS_OBJECT_PATH,
        Duration::from_millis(5000),
    );

    if options.basic {
        println!("\n=== Running 'basic' tests... ===\n");
        test_method!(proxy "GetServerInformation" ServerInfoMessageOutput,ServerInfoMessageOutputType);
        test_method!(proxy "GetCapabilities" ServerCapsMessageOutput,ServerCapsMessageOutputType);
    }

    if options.basic_notification {
        println!("\n=== Running 'basic-notification' tests... ===\n");

        let output_opt = send_notification(
            &mut proxy,
            NotifyMessageInput {
                app_name: String::from("freedesktop-notification-tester"),
                replaces_id: 0,
                app_icon: String::new(),
                summary: String::from("This is a summary."),
                body: String::from("This is a message body.\nClose me to continue the tests."),
                actions: Vec::new(),
                hints: HashMap::new(),
                expire_timeout: 0,
            },
        );

        if let Some(output) = output_opt {
            println!(
                "  Successfully sent a notification, which acquire id '{}'.",
                output.id
            );

            let result =
                wait_for_notification_close(&mut proxy, output.id, Duration::from_secs(20));

            match &result {
                Ok(1) => println!("  The notification has expired (timed out)."),
                Ok(2) => println!("  The notification was dismissed by the user."),
                Ok(3) => println!("  The notification was closed by a DBus call."),
                Ok(u32::MAX) => {
                    println!("  Did not receive a 'NotificationClosed' signal after 20 seconds...")
                }
                Ok(reason_id) => println!(
                    "  The notification was closed via an unknown method (reason: {}).",
                    reason_id
                ),
                Err(error) => {
                    eprintln!(
                        "  Failed to wait for a notification close signal: {}",
                        error
                    )
                }
            }

            match result {
                Ok(1) => println!(
                    "The 'NotificationClosed' signal is being properly sent by the server. However, the server did not properly handle 'expire_timeout' = 0 and expired the notification."
                ),
                Ok(2 | 3) => println!(
                    "The 'NotificationClosed' signal is being properly sent by the server."
                ),
                _ => {
                    println!("It's possible that the server did not report a closed notification.")
                }
            }
        } else {
            println!("Failed to send a notification.");
        }
    }

    if options.expire_timeout {
        println!("\n=== Running 'expire-timeout' tests... ===\n");

        let mut hints = HashMap::new();
        hints.insert(String::from("transient"), as_variant!(true));

        let output_opt = send_notification(
            &mut proxy,
            NotifyMessageInput {
                app_name: String::from("freedesktop-notification-tester"),
                replaces_id: 0,
                app_icon: String::new(),
                summary: String::from("expire-timeout"),
                body: String::from("This notification should expire in 2 seconds."),
                actions: Vec::new(),
                hints: hints,
                expire_timeout: 2000,
            },
        );

        if let Some(output) = output_opt {
            println!(
                "  Successfully sent a notification, which acquire id '{}'.",
                output.id
            );

            let result = wait_for_notification_close(&mut proxy, output.id, Duration::from_secs(3));

            match &result {
                Ok(1) => println!("  The notification has expired (timed out)."),
                Ok(_) => {
                    println!("  The notification did not close by timeout, as it was supposed to.")
                }
                Err(error) => {
                    eprintln!(
                        "  Failed to wait for a notification close signal: {}",
                        error
                    )
                }
            }

            if let Ok(1) = result {
                println!("The server has properly handled the 'expire_timeout' argument.");
            }
        } else {
            println!("Failed to send a notification.");
        }
    }

    if options.multiple_notifications {
        println!("\n=== Running 'multiple-notifications' tests... ===\n");

        let mut hints = HashMap::new();
        hints.insert(String::from("transient"), as_variant!(true));

        let output_1_opt = send_notification(
            &mut proxy,
            NotifyMessageInput {
                app_name: String::from("freedesktop-notification-tester"),
                replaces_id: 0,
                app_icon: String::new(),
                summary: String::from("multiple-notifications"),
                body: String::from("This is the #1 notification.\nIt will expire in 2 seconds."),
                actions: Vec::new(),
                hints: hints,
                expire_timeout: 2000,
            },
        );

        let mut hints = HashMap::new();
        hints.insert(String::from("transient"), as_variant!(true));

        let output_2_opt = send_notification(
            &mut proxy,
            NotifyMessageInput {
                app_name: String::from("freedesktop-notification-tester"),
                replaces_id: 0,
                app_icon: String::new(),
                summary: String::from("multiple-notifications"),
                body: String::from("This is the #2 notification.\nIt will expire in 3 seconds."),
                actions: Vec::new(),
                hints: hints,
                expire_timeout: 3000,
            },
        );

        let mut hints = HashMap::new();
        hints.insert(String::from("transient"), as_variant!(true));

        let output_3_opt = send_notification(
            &mut proxy,
            NotifyMessageInput {
                app_name: String::from("freedesktop-notification-tester"),
                replaces_id: 0,
                app_icon: String::new(),
                summary: String::from("multiple-notifications"),
                body: String::from("This is the #3 notification.\nIt will expire in 4 seconds."),
                actions: Vec::new(),
                hints: hints,
                expire_timeout: 4000,
            },
        );

        let ids = [output_1_opt, output_2_opt, output_3_opt].map(|opt| opt.unwrap().id);
        for (idx, id) in ids.into_iter().enumerate() {
            let out_res = wait_for_notification_close(&mut proxy, id, Duration::from_secs(5));
            if let Ok(1) = out_res {
                println!("  Notification #{} has closed with success.", idx + 1);
            } else {
                println!("  Notification #{} failed to close with success.", idx + 1);
            }
        }
    }

    Ok(())
}

fn send_notification(
    proxy: &mut dbus::blocking::Proxy<'_, &Connection>,
    parameters: NotifyMessageInput,
) -> Option<NotifyMessageOutput> {
    let r: Result<NotifyMessageOutputType, dbus::Error> = proxy.method_call(
        NOTIFICATION_BUS_INTERFACE_NAME,
        "Notify",
        NotifyMessageInputType::from(parameters),
    );
    match r {
        Ok(response) => Some(NotifyMessageOutput::from(response)),
        Err(error) => {
            eprintln!("Error sending notification: {}", error);
            None
        }
    }
}

fn wait_for_notification_close(
    proxy: &mut dbus::blocking::Proxy<'_, &Connection>,
    notification_id: u32,
    max_time_to_wait: Duration,
) -> Result<u32, dbus::Error> {
    use std::sync::{Arc, Mutex};

    let condition = Arc::new(Mutex::new(u32::MAX));
    let condition_callback = Arc::clone(&condition);

    let callback_id = proxy.match_signal(
        move |signal: NotificationClosedSignal, _: &Connection, _: &dbus::Message| {
            if signal.id == notification_id {
                *condition_callback.lock().unwrap() = signal.reason;
            }

            signal.id != notification_id
        },
    )?;

    let start_time = std::time::Instant::now();

    println!("Now waiting for the notification to close and the server to report so...");
    while start_time.elapsed() <= max_time_to_wait && *condition.lock().unwrap() == u32::MAX {
        proxy.connection.process(Duration::from_millis(100))?;
    }

    let result = *condition.lock().unwrap();
    if result == u32::MAX {
        proxy.match_stop(callback_id, true)?;
    }

    Ok(result)
}
