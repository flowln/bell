use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::Connection;
use wayland_client::backend::ObjectId;

mod configuration;
mod dbus;
mod icon;
mod lock;
mod notification;
mod render;
mod signal;
mod wayland;

use crate::dbus as _dbus;
use configuration::Configuration;
use icon::retrieve_app_icon;
use render::render::Renderer;
use render::{Attrs, Metrics};

use configuration::{GrowthDirection, OutputConfiguration};
use notification::{
    Notification, SurfaceProcessingOutput, notification_manager_read, notification_manager_write,
};

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
unsafe fn sigint_handler() {
    EXIT_REQUESTED.store(true, Ordering::Relaxed);
}

static TEXT_PLACEHOLDER: &'static str = "<<placeholder>>";
fn render_notification(
    renderer: &mut Renderer,
    notification: &Notification,
    spec: &OutputConfiguration,
) -> Option<()> {
    renderer.clear(None);

    let mut padding_x = 10usize;
    let mut padding_y = 10usize;

    if let Some(border_size) = spec.border_size
        && border_size != 0
    {
        renderer.draw_border(border_size, spec.border_radius, spec.border_color?);

        padding_x += border_size;
        padding_y += border_size;
    }

    let mut icon_size = 0;
    if let Some(app_icon) = &notification.app_icon {
        let preferred_icon_size = icon::IconSize { size: 16, scale: 1 };
        let icon_information = retrieve_app_icon(
            app_icon.as_str(),
            spec.icon_theme.as_deref(),
            preferred_icon_size,
        )
        .unwrap();

        let size = icon_information
            .icon_size
            .unwrap_or(preferred_icon_size)
            .scaled_size();

        let x_position = 0i32 - padding_x as i32 - size as i32;
        let y_position = padding_y as i32;

        match icon_information.file_type {
            icon::IconFileType::PNG => {
                if let Err(error) =
                    renderer.draw_png(x_position, y_position, size, size, &icon_information.path)
                {
                    eprintln!("Error drawing PNG icon: {}", error);
                }
            }
            _ => {}
        }

        icon_size = size;
    }

    let mut image_size = 0;
    if let Some(image_data) = notification.image_data.as_ref() {
        let remaining_size = usize::min(
            renderer.width - 2 * padding_x,
            renderer.height - icon_size - 2 * padding_y,
        );

        let effective_size = remaining_size.min(64);
        let (width, height) = (effective_size, effective_size);

        let x_position = 0i32 - padding_x as i32 - width as i32;
        let y_position = 0i32 - padding_y as i32 - height as i32;

        renderer.draw_image(x_position, y_position, width, height, image_data);

        image_size = effective_size;
    }

    use cosmic_text::Family;
    let mut default_text_opts = render::text::Attrs::new();
    if let Some(font_family) = &spec.font_family {
        let family = match font_family.as_str() {
            "Serif" | "serif" => Family::Serif,
            "SansSerif" | "sansserif" => Family::SansSerif,
            "Cursive" | "cursive" => Family::Cursive,
            "Fantasy" | "fantasy" => Family::Fantasy,
            "Monospace" | "monospace" => Family::Monospace,
            family_name => Family::Name(family_name),
        };

        default_text_opts = default_text_opts.family(family);
    }

    let mut text_span_fragment_holder = Vec::<String>::new();
    let mut text_span = Vec::<(&str, Attrs)>::new();

    spec.get_message_layout(|text_fragment, font_size, text_color| {
        // NOTE: Arbitrary values to make a reasonable line height.
        let metrics = Metrics::new(font_size, font_size + 6.0f32.min(font_size * 0.3));
        let attrs = default_text_opts.clone().metrics(metrics).color(text_color);

        match text_fragment {
            "app_name" => text_span.push((notification.app_name.as_str(), attrs)),
            "summary" => text_span.push((notification.summary.as_str(), attrs)),
            "body" => text_span.push((notification.body.as_str(), attrs)),
            fragment => {
                text_span_fragment_holder.push(String::from(fragment));
                text_span.push((TEXT_PLACEHOLDER, attrs));
            }
        }
    });

    let mut holder_it = text_span_fragment_holder.iter();
    text_span.iter_mut().for_each(|(text, _)| {
        if *text == TEXT_PLACEHOLDER {
            *text = holder_it.next().unwrap().as_str();
        }
    });

    let remaining_width = renderer.width - 2 * padding_x - image_size.max(icon_size);
    let remaining_height = renderer.height - 2 * padding_y;
    renderer.draw_text_spans(
        text_span,
        padding_x as i32,
        padding_y as i32,
        remaining_width,
        remaining_height,
        default_text_opts,
    );

    Some(())
}

fn with_offset<F>(offset: &mut i32, spec: &OutputConfiguration, mut closure: F) -> Option<()>
where
    F: FnMut(i32) -> Option<()>,
{
    let direction = spec.direction?;
    let margins = spec.margins?;

    match direction {
        GrowthDirection::Up => *offset += margins.bottom,
        GrowthDirection::Right => *offset += margins.left,
        GrowthDirection::Down => *offset += margins.top,
        GrowthDirection::Left => *offset += margins.right,
    }

    closure(*offset);

    match direction {
        GrowthDirection::Up => *offset += spec.height? + margins.top,
        GrowthDirection::Right => *offset += spec.width? + margins.right,
        GrowthDirection::Down => *offset += spec.height? + margins.bottom,
        GrowthDirection::Left => *offset += spec.width? + margins.left,
    }

    Some(())
}

fn process_surface(
    renderers_for_surfaces: &mut HashMap<ObjectId, Renderer>,
    offset_per_output: &mut HashMap<String, i32>,
    notification: &Notification,
    surface_id: &ObjectId,
) -> SurfaceProcessingOutput {
    let mut wayland_state = wayland::wayland_state_write();

    let surface = wayland_state.get_surface(surface_id);
    if surface.is_none() {
        return SurfaceProcessingOutput::NoSurface;
    }

    let surface = surface.unwrap();
    if surface.is_destroyed() {
        return SurfaceProcessingOutput::SurfaceDestroyed;
    }

    let output_name = surface.output_name.clone();
    let will_destroy = surface.will_destroy_later() || notification.has_timed_out();

    let output_spec = notification.get_output_spec(&output_name).unwrap();
    let try_rendering = |offset: i32| {
        if notification.is_dirty || surface.is_dirty {
            if !renderers_for_surfaces.contains_key(surface_id) {
                renderers_for_surfaces.insert(
                    surface_id.clone(),
                    Renderer::new(
                        surface.surface_width as usize,
                        surface.surface_height as usize,
                        output_spec.background_color?,
                    ),
                );
            }

            {
                use wayland::SurfaceBackend;
                let SurfaceBackend::Wlr(wlr_surface) = &mut surface.backend;

                let original_margins = output_spec.margins?;
                let margins = match output_spec.direction? {
                    GrowthDirection::Up => original_margins.with_bottom(offset),
                    GrowthDirection::Right => original_margins.with_left(offset),
                    GrowthDirection::Down => original_margins.with_top(offset),
                    GrowthDirection::Left => original_margins.with_right(offset),
                };

                wlr_surface.set_margin(margins.top, margins.right, margins.bottom, margins.left);
            }

            let renderer = renderers_for_surfaces.get_mut(surface_id).unwrap();

            renderer.set_buffer_scale(surface.get_buffer_scale());

            render_notification(renderer, notification, output_spec);
            surface.write(renderer.get_backing_store());
        }

        Some(())
    };

    // We'll destroy it at the end of the current cycle, but for now we keep it alive so
    // that it correctly transfers surface focus when unmapping.
    if !will_destroy {
        if !offset_per_output.contains_key(&output_name) {
            offset_per_output.insert(output_name.clone(), 0);
        }
        let offset = offset_per_output.get_mut(&output_name).unwrap();

        with_offset(offset, output_spec, try_rendering);
    }

    return SurfaceProcessingOutput::Continue;
}

struct ApplicationOptions {
    config_path: Option<String>,
    ephemeral: bool,
}

impl Default for ApplicationOptions {
    fn default() -> Self {
        ApplicationOptions {
            ephemeral: false,
            config_path: None,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args();

    let mut options = ApplicationOptions::default();
    let mut args_iter = arguments.into_iter();
    args_iter.next(); // Skip application name argument.
    while let Some(argument) = args_iter.next() {
        match argument.as_str() {
            "-c" | "--config" => {
                let config_path = args_iter
                    .next()
                    .expect("Configuration file argument specified without a value after it.");
                options.config_path = Some(config_path);
            }
            "--ephemeral" => options.ephemeral = true,
            "--help" => {
                println!("bell: a lightweight notification daemon");
                println!("");
                println!("usage: bell [--ephemeral]");
                println!("");
                println!("Available options are:");
                println!(
                    "  --ephemeral: Exit application after handling at least one notification."
                );
                println!("  --help:      Show this help menu.");
                println!("");
                println!("For more information, access the website:");
                println!("https://github.com/flowln/bell");

                return Ok(());
            }
            unrecognized => eprintln!("Unrecognized option '{}'", unrecognized),
        }
    }

    use std::path::PathBuf;
    let configuration = match options.config_path {
        None => Configuration::from_default_paths().unwrap_or_else(|_err| {
            eprintln!(
                "Could not find a valid configuration file! Using the default configuration."
            );
            Configuration::default()
        }),
        Some(path_str) => Configuration::from_file(PathBuf::from(path_str).as_path())?,
    };
    let event_handler = configuration.get_event_handler();

    {
        let mut notification_manager = notification_manager_write();
        notification_manager.set_configuration(configuration);
    }

    let mut dbus_connection = _dbus::create_connection().unwrap();

    let mut event_queue = {
        let conn = Connection::connect_to_env().unwrap();

        let display = conn.display();

        let mut event_queue = conn.new_event_queue();
        let queue_handle = event_queue.handle();

        let _registry = display.get_registry(&queue_handle, ());

        let mut wayland_state = wayland::wayland_state_write();
        wayland_state.initialize(queue_handle, &display);

        while wayland_state.pending_data_amount != 0 {
            event_queue.roundtrip(&mut wayland_state).unwrap();
        }

        event_queue
    };

    let mut socket_handler = wayland::SocketManager::new(&mut event_queue);
    let mut renderers_for_surfaces = HashMap::<wayland::SurfaceID, Renderer>::new();

    let mut manager_callback =
        |surface_id: &wayland::SurfaceID,
         notification: &Notification,
         offset_per_output: &mut HashMap<String, i32>| {
            process_surface(
                &mut renderers_for_surfaces,
                offset_per_output,
                notification,
                surface_id,
            )
        };

    signal::install_signal_handler(signal::PosixSignal::SIGINT, sigint_handler);

    while !EXIT_REQUESTED.load(Ordering::Relaxed) {
        let event_queue = {
            let mut wayland_state = wayland::wayland_state_write();
            let trigger_queue = wayland_state.consume_trigger_events();

            trigger_queue
                .map(|(id, trigger_list)| (id, trigger_list.iter().map(&event_handler).collect()))
                .collect()
        };

        let closed_notifications = {
            let mut notification_manager = notification_manager_write();
            notification_manager.process_active_notifications(&event_queue, &mut manager_callback)
        };

        {
            let mut wayland_state = wayland::wayland_state_write();
            wayland_state.destroy_scheduled_surfaces();
        }

        for (id, reason) in closed_notifications {
            _dbus::emit_notification_closed(&mut dbus_connection, id, reason as u32).unwrap();
        }

        socket_handler.handle(50);
        dbus_connection
            .process(std::time::Duration::from_millis(50))
            .unwrap();

        if options.ephemeral {
            let notification_manager = notification_manager_read();
            if notification_manager.has_had_any_notification()
                && !notification_manager.has_any_active_notification()
            {
                break;
            }
        }
    }

    Ok(())
}
