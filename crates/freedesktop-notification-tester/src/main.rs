use dbus::blocking::Connection;

use std::collections::HashMap;
use std::time::Duration;

mod types;
use types::*;

#[derive(Debug)]
struct Error(String);

impl From<dbus::Error> for Error {
    fn from(err: dbus::Error) -> Self {
        Error(format!(
            "DBus error: name={} message={}",
            err.name().unwrap_or_default(),
            err.message().unwrap_or_default()
        ))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
            let error = response_res.unwrap_err();
            if error.name().unwrap_or_default() == "org.freedesktop.DBus.Error.NameHasNoOwner" {
                eprintln!(
                    "No program has ownership of the notification bus. Is a server running?\n{}",
                    error
                );

                std::process::exit(1);
            }

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
    ($value:ident) => {
        dbus::arg::Variant(Box::new($value.clone()) as Box<dyn dbus::arg::RefArg>)
    };
}

macro_rules! add_application_options {
    ($($name:ident:$default:literal) +) => {
        struct ApplicationOptions {
            $(
                $name: bool,
            )+
        }

        impl Default for ApplicationOptions {
            fn default() -> Self {
                ApplicationOptions {
                    $(
                        $name: $default,
                    )+
                }
            }
        }

        impl ApplicationOptions {
            pub fn enable_all(&mut self) {
                $(
                    self.$name = true;
                )+
            }

            pub fn parse_option(&mut self, option: &str) {
                match option[2..].replace("-", "_").as_str() {
                    $(
                        stringify!($name) => self.$name = true,
                    )+
                    unrecognized => eprintln!("Unrecognized option '{}'", unrecognized),
                }
            }
        }
    };
}

add_application_options!(
    basic:true
    basic_notification:false
    expire_timeout:false
    multiple_notifications:false
    body_markup:false
    close_notification:false
    notification_sounds:false
    urgency:false
);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = ApplicationOptions::default();

    let arguments = std::env::args();
    let mut args_iter = arguments.into_iter();
    args_iter.next(); // Skip application name argument.

    while let Some(argument) = args_iter.next() {
        match argument.as_str() {
            "--all" => options.enable_all(),
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
                    "  --body-markup: Test support for markup in body messages (bold, italic, underline).",
                    "  --close-notification: Test support for closing a notification via DBus.",
                    "  --notification-sounds: Test support for sound features (default, suppress, sound-file, sound-name).",
                    "  --urgency: Test support for the urgengy hint in notifications.",
                ].join("\n"));
                println!("  --help:      Show this help menu.");
                println!("");
                println!("For more information, access the website:");
                println!("https://github.com/flowln/bell");

                return Ok(());
            }
            "--no-basic" => options.basic = false,
            option => options.parse_option(option),
        }
    }

    let conn = Connection::new_session()?;

    let mut proxy = conn.with_proxy(
        NOTIFICATION_BUS_INTERFACE_NAME,
        NOTIFICATION_BUS_OBJECT_PATH,
        Duration::from_millis(5000),
    );

    if options.basic {
        run_test("basic", || {
            test_method!(proxy "GetServerInformation" ServerInfoMessageOutput,ServerInfoMessageOutputType);
            test_method!(proxy "GetCapabilities" ServerCapsMessageOutput,ServerCapsMessageOutputType);

            Ok(true)
        });
    }

    if options.basic_notification {
        run_test("basic-notification", || {
            let output = send_notification(
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
            )?;

            println!(
                "  Successfully sent a notification, which acquire id '{}'.",
                output.id
            );

            let result =
                wait_for_notification_close(&mut proxy, output.id, Duration::from_secs(20))?;

            match result {
                1 => println!("  The notification has expired (timed out)."),
                2 => println!("  The notification was dismissed by the user."),
                3 => println!("  The notification was closed by a DBus call."),
                u32::MAX => {
                    println!(
                        "  Did not receive a 'NotificationClosed' signal after 20 seconds..."
                    )
                }
                reason_id => println!(
                    "  The notification was closed via an unknown method (reason: {}).",
                    reason_id
                ),
            }

            match result {
                1 => {
                    println!(
                        "The 'NotificationClosed' signal is being properly sent by the server. However, the server did not properly handle 'expire_timeout' = 0 and expired the notification."
                    );

                    Ok(true)
                }
                2 | 3 => {
                    println!(
                        "The 'NotificationClosed' signal is being properly sent by the server."
                    );

                    Ok(true)
                }
                _ => {
                    println!(
                        "It's possible that the server did not report a closed notification."
                    );

                    Ok(false)
                }
            }
        });
    }

    if options.expire_timeout {
        run_test("expire-timeout", || {
            let mut hints = HashMap::new();
            hints.insert(String::from("transient"), as_variant!(true));

            let output = send_notification(
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
            )?;

            println!(
                "  Successfully sent a notification, which acquire id '{}'.",
                output.id
            );

            let result =
                wait_for_notification_close(&mut proxy, output.id, Duration::from_secs(3))?;

            match &result {
                1 => println!("  The notification has expired (timed out)."),
                _ => {
                    println!(
                        "  The notification did not close by timeout, as it was supposed to."
                    )
                }
            }

            if result == 1 {
                println!("The server has properly handled the 'expire_timeout' argument.");

                Ok(true)
            } else {
                Ok(false)
            }
        });
    }

    if options.multiple_notifications {
        run_test("multiple-notifications", || {
            let mut hints = HashMap::new();
            hints.insert(String::from("transient"), as_variant!(true));

            let output_1 = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("multiple-notifications"),
                    body: String::from(
                        "This is the #1 notification.\nIt will expire in 2 seconds.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 2000,
                },
            )?;

            let mut hints = HashMap::new();
            hints.insert(String::from("transient"), as_variant!(true));

            let output_2 = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("multiple-notifications"),
                    body: String::from(
                        "This is the #2 notification.\nIt will expire in 3 seconds.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 3000,
                },
            )?;

            let mut hints = HashMap::new();
            hints.insert(String::from("transient"), as_variant!(true));

            let output_3 = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("multiple-notifications"),
                    body: String::from(
                        "This is the #3 notification.\nIt will expire in 4 seconds.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 4000,
                },
            )?;

            let mut status = true;

            let ids = [output_1, output_2, output_3].map(|opt| opt.id);
            for (idx, id) in ids.into_iter().enumerate() {
                let out = wait_for_notification_close(&mut proxy, id, Duration::from_secs(5))?;
                if out == 1 {
                    println!("  Notification #{} has closed with success.", idx + 1);
                } else {
                    println!("  Notification #{} failed to close with success.", idx + 1);
                    status = false;
                }
            }

            Ok(status)
        });
    }

    if options.body_markup {
        run_test("body-markup", || {
            let output = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("body-markup"),
                    body: String::from(
                        "Close this notification to continue.\nThis is <b>bold</b>, <i>italic</i> and <u>underline</u>.\n<b><i><u>This is everything at once.</u></i></b>",
                    ),
                    actions: Vec::new(),
                    hints: HashMap::new(),
                    expire_timeout: 0,
                },
            )?;

            let _ = wait_for_notification_close(&mut proxy, output.id, Duration::from_secs(20));

            Ok(true)
        });
    }

    if options.close_notification {
        run_test("close-notification", || {
            let output = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("close-notification"),
                    body: String::from(
                        "This notification should close on its own after 2 seconds.",
                    ),
                    actions: Vec::new(),
                    hints: HashMap::new(),
                    expire_timeout: 0,
                },
            )?;

            println!("Notification was sent, waiting two seconds before sending a close request.");
            std::thread::sleep(Duration::from_millis(2000));

            let id = output.id;
            match wait_for_notification_close_with_action(
                &mut proxy,
                id,
                Duration::from_secs(5),
                |proxy| {
                    println!("Sending close request for notification with id '{}'.", id);
                    let _r: Result<CloseNotificationInputType, dbus::Error> = proxy.method_call(
                        NOTIFICATION_BUS_INTERFACE_NAME,
                        "CloseNotification",
                        (id,),
                    );
                },
            )? {
                3 => {
                    println!("  Notification was successfully closed by a DBus call.");

                    Ok(true)
                }
                _ => {
                    println!("  Failed to close notification via DBus call. ");

                    Ok(false)
                }
            }
        });
    }

    if options.notification_sounds {
        run_test("notification-sounds", || {
            {
                let _ = send_notification(
                    &mut proxy,
                    NotifyMessageInput {
                        app_name: String::from("freedesktop-notification-tester"),
                        replaces_id: 0,
                        app_icon: String::new(),
                        summary: String::from("notification-sounds"),
                        body: String::from(
                            "This notification should play the default sound from the server configuration (if any).",
                        ),
                        actions: Vec::new(),
                        hints: HashMap::new(),
                        expire_timeout: 1000,
                    },
                )?;

                println!("Notification was sent, waiting a second before sending the next one.");
                std::thread::sleep(Duration::from_millis(1000));
            }

            {
                let mut hints = HashMap::new();
                hints.insert(String::from("suppress-sound"), as_variant!(true));

                let _ = send_notification(
                    &mut proxy,
                    NotifyMessageInput {
                        app_name: String::from("freedesktop-notification-tester"),
                        replaces_id: 0,
                        app_icon: String::new(),
                        summary: String::from("notification-sounds"),
                        body: String::from(
                            "This notification should not a sound at all (suppress-sound = true).",
                        ),
                        actions: Vec::new(),
                        hints: hints,
                        expire_timeout: 1000,
                    },
                )?;

                println!("Notification was sent, waiting a second before sending the next one.");
                std::thread::sleep(Duration::from_millis(1000));
            }

            {
                let mut hints = HashMap::new();
                let sound_file =
                    String::from("/usr/share/sounds/freedesktop/stereo/audio-test-signal.oga");
                hints.insert(String::from("sound-file"), as_variant!(sound_file));

                let _ = send_notification(
                    &mut proxy,
                    NotifyMessageInput {
                        app_name: String::from("freedesktop-notification-tester"),
                        replaces_id: 0,
                        app_icon: String::new(),
                        summary: String::from("notification-sounds"),
                        body: format!(
                            "This notification should play a test sound (using sound-file):\n{}",
                            sound_file,
                        ),
                        actions: Vec::new(),
                        hints: hints,
                        expire_timeout: 2000,
                    },
                )?;

                println!("Notification was sent, waiting two seconds before sending the next one.");
                std::thread::sleep(Duration::from_millis(2000));
            }

            {
                let mut hints = HashMap::new();
                let sound_name = String::from("audio-test-signal");
                hints.insert(String::from("sound-name"), as_variant!(sound_name));

                let _ = send_notification(
                    &mut proxy,
                    NotifyMessageInput {
                        app_name: String::from("freedesktop-notification-tester"),
                        replaces_id: 0,
                        app_icon: String::new(),
                        summary: String::from("notification-sounds"),
                        body: format!(
                            "This notification should play a test sound too (using sound-name):\n{}", sound_name,
                        ),
                        actions: Vec::new(),
                        hints: hints,
                        expire_timeout: 2000,
                    },
                )?;

                println!("Notification was sent, waiting two seconds before ending the test.");
                std::thread::sleep(Duration::from_millis(2000));
            }

            Ok(true)
        });
    }

    if options.urgency {
        run_test("urgency", || {
            let mut hints = HashMap::new();
            hints.insert(String::from("urgency"), as_variant!(0u8));

            let _ = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("urgency"),
                    body: String::from(
                        "This is a Low urgency notification.\nIt will expire in 4s.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 4000,
                },
            )?;

            let mut hints = HashMap::new();
            hints.insert(String::from("urgency"), as_variant!(1u8));

            let _ = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("urgency"),
                    body: String::from(
                        "This is a Normal urgency notification.\nIt will expire in 4s.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 4000,
                },
            )?;

            let mut hints = HashMap::new();
            hints.insert(String::from("urgency"), as_variant!(2u8));

            let critical = send_notification(
                &mut proxy,
                NotifyMessageInput {
                    app_name: String::from("freedesktop-notification-tester"),
                    replaces_id: 0,
                    app_icon: String::new(),
                    summary: String::from("urgency"),
                    body: String::from(
                        "This is a Critical urgency notification.\nIt should not expire on its own.\nDismiss it to continue.",
                    ),
                    actions: Vec::new(),
                    hints: hints,
                    expire_timeout: 4000,
                },
            )?;

            println!(
                "Sent all three notifications. Will check if the critical notification expires..."
            );

            let id = critical.id;
            let result = wait_for_notification_close(&mut proxy, id, Duration::from_secs(5))?;
            if result == 1 {
                println!("  A critical notification has expired, which should not happen.");

                return Ok(false);
            } else {
                println!("  The notification has not expired, as was expected.");
            }

            let _ = wait_for_notification_close_with_action(
                &mut proxy,
                id,
                Duration::from_secs(5),
                |proxy| {
                    println!("Sending close request for notification with id '{}'.", id);
                    let _r: Result<CloseNotificationInputType, dbus::Error> = proxy
                        .method_call(
                            NOTIFICATION_BUS_INTERFACE_NAME,
                            "CloseNotification",
                            (id,),
                        );
                },
            )?;

            Ok(true)
        });
    }

    Ok(())
}

fn run_test<F>(test_name: &str, mut test_function: F)
where
    F: FnMut() -> Result<bool, Error>,
{
    println!("\n=== Running '{}' tests... ===\n", test_name);

    match test_function() {
        Ok(true) => println!("+++ Test '{}' succeeded.", test_name),
        Ok(false) => println!("--- Test '{}' failed.", test_name),
        Err(error) => eprintln!("!!! Test '{}' failed with an error: {}", test_name, error.0),
    }

    std::thread::sleep(Duration::from_millis(200));
}

fn send_notification(
    proxy: &mut dbus::blocking::Proxy<'_, &Connection>,
    parameters: NotifyMessageInput,
) -> Result<NotifyMessageOutput, Error> {
    let r: Result<NotifyMessageOutputType, dbus::Error> = proxy.method_call(
        NOTIFICATION_BUS_INTERFACE_NAME,
        "Notify",
        NotifyMessageInputType::from(parameters),
    );
    match r {
        Ok(response) => Ok(NotifyMessageOutput::from(response)),
        Err(error) => Err(Error::from(error)),
    }
}

fn wait_for_notification_close(
    proxy: &mut dbus::blocking::Proxy<'_, &Connection>,
    notification_id: u32,
    max_time_to_wait: Duration,
) -> Result<u32, Error> {
    wait_for_notification_close_with_action(proxy, notification_id, max_time_to_wait, |_| {})
}

fn wait_for_notification_close_with_action<F>(
    proxy: &mut dbus::blocking::Proxy<'_, &Connection>,
    notification_id: u32,
    max_time_to_wait: Duration,
    mut pre_action: F,
) -> Result<u32, Error>
where
    F: FnMut(&mut dbus::blocking::Proxy<'_, &Connection>) -> (),
{
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
    pre_action(proxy);
    while start_time.elapsed() <= max_time_to_wait && *condition.lock().unwrap() == u32::MAX {
        proxy.connection.process(Duration::from_millis(100))?;
    }

    let result = *condition.lock().unwrap();
    if result == u32::MAX {
        proxy.match_stop(callback_id, true)?;
    }

    Ok(result)
}
