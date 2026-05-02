use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::Connection;
use wayland_client::backend::ObjectId;

mod configuration;
mod dbus;
mod lock;
mod notification;
mod render;
mod signal;
mod wayland;

use configuration::Configuration;
use crate::dbus as _dbus;
use render::Color;
use render::render::Renderer;

use configuration::{GrowthDirection, OutputConfiguration};
use notification::{Notification, SurfaceProcessingOutput, notification_manager_read, notification_manager_write};


static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
unsafe fn sigint_handler() {
    EXIT_REQUESTED.store(true, Ordering::Relaxed);
}


fn render(renderer: &mut Renderer, notification: &Notification, spec: &OutputConfiguration) {
    let mut text_opts = render::text::TextRenderOptions::new();
    text_opts.font_size = 20.;

    renderer.draw_text(
        &notification.app_name,
        10,
        30,
        Color::rgba(0x00, 0x00, 0x00, 0xFF),
        text_opts,
    );

    let mut text_opts = render::text::TextRenderOptions::new();
    text_opts.font_size = 12.;

    renderer.draw_text(
        &notification.summary,
        10,
        80,
        Color::rgba(0xFF, 0xFF, 0xFF, 0xFF),
        text_opts,
    );

    if let Some(border_color) = spec.border_color {
        renderer.draw_border(spec.border_size.unwrap(), border_color);
    }
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
    let will_destroy = surface.will_destroy_later();

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

            use wayland::SurfaceBackend;
            if let SurfaceBackend::Wlr(wlr_surface) = &mut surface.backend {
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
            renderer.clear(None);

            render(renderer, notification, output_spec);
            surface.write(renderer.get_backend());
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

fn main() {
    let configuration = Configuration::from_default_paths();
    if let Err(failed_paths) = &configuration {
        for (kind, path) in failed_paths {
            eprintln!("{}: {}", kind.to_string(), path);
        }

        eprintln!("Could not find a valid configuration file! Using the default configuration.");
    }
    let configuration = configuration.unwrap_or_default();
    let event_handler = configuration.get_event_handler();

    {
        let mut notification_manager = notification_manager_write();
        notification_manager.set_configuration(configuration);
    }

    let dbus_connection = _dbus::create_server().unwrap();

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

        {
            let mut notification_manager = notification_manager_write();
            notification_manager.process_active_notifications(&event_queue, &mut manager_callback);
        }

        {
            let mut wayland_state = wayland::wayland_state_write();
            wayland_state.destroy_scheduled_surfaces();
        }

        socket_handler.handle(50);
        dbus_connection.process(std::time::Duration::from_millis(50)).unwrap();
    }
}
