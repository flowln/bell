use std::cell::Cell;
use std::collections::HashMap;
use std::time;

use wayland_client::Connection;
use wayland_client::backend::ObjectId;

mod notification;
mod render;
mod wayland;

use render::Color;
use render::render::Renderer;
use wayland::wayland as backend;

use notification::{GrowthDirection, OutputSpecifier, Notification, NotificationManager, SurfaceProcessingOutput};

fn render(renderer: &mut Renderer, notification: &Notification, spec: &OutputSpecifier) {
    let mut text_opts = render::text::TextRenderOptions::new();
    text_opts.font_size = 20.;

    renderer.draw_text(
        &notification.title,
        10,
        30,
        Color::rgba(0x00, 0x00, 0x00, 0xFF),
        text_opts,
    );

    let mut text_opts = render::text::TextRenderOptions::new();
    text_opts.font_size = 12.;

    renderer.draw_text(
        &notification.message,
        10,
        80,
        Color::rgba(0xFF, 0xFF, 0xFF, 0xFF),
        text_opts,
    );

    if let Some(border_color) = spec.border_color {
        renderer.draw_border(spec.border_size, border_color);
    }
}

fn with_offset<F>(offset: &mut i32, spec: &OutputSpecifier, mut closure: F)
where
    F: FnMut(i32) -> (),
{
    match spec.direction {
        GrowthDirection::Up => *offset += spec.margins.bottom,
        GrowthDirection::Right => *offset += spec.margins.left,
        GrowthDirection::Down => *offset += spec.margins.top,
        GrowthDirection::Left => *offset += spec.margins.right,
    }

    closure(*offset);

    match spec.direction {
        GrowthDirection::Up => *offset += spec.height + spec.margins.top,
        GrowthDirection::Right => *offset += spec.width + spec.margins.right,
        GrowthDirection::Down => *offset += spec.height + spec.margins.bottom,
        GrowthDirection::Left => *offset += spec.width + spec.margins.left,
    }
}

fn process_surface(
    data: &mut Cell<backend::GlobalData>,
    renderers_for_surfaces: &mut HashMap<ObjectId, Renderer>,
    offset_per_output: &mut HashMap<String, i32>,
    notification: &Notification,
    surface_id: &ObjectId,
) -> SurfaceProcessingOutput {
    let global_data = data.get_mut();

    let surface = global_data.get_surface(surface_id);
    if surface.is_none() {
        return SurfaceProcessingOutput::NoSurface;
    }

    let surface = surface.unwrap();
    if surface.is_destroyed() {
        return SurfaceProcessingOutput::SurfaceDestroyed;
    }

    let output_name = surface.output_name.clone();

    let output_spec = notification.get_output_spec(&output_name).unwrap();
    let try_rendering = |offset: i32| {
        if notification.is_dirty || surface.is_dirty {
            if !renderers_for_surfaces.contains_key(surface_id) {
                renderers_for_surfaces.insert(
                    surface_id.clone(),
                    Renderer::new(
                        surface.surface_width as usize,
                        surface.surface_height as usize,
                        output_spec.background_color,
                    ),
                );
            }

            use backend::SurfaceBackend;
            if let SurfaceBackend::Wlr(wlr_surface) = &mut surface.backend {
                let margins = match output_spec.direction {
                    GrowthDirection::Up => output_spec.margins.with_bottom(offset),
                    GrowthDirection::Right => output_spec.margins.with_left(offset),
                    GrowthDirection::Down => output_spec.margins.with_top(offset),
                    GrowthDirection::Left => output_spec.margins.with_right(offset),
                };

                wlr_surface.set_margin(margins.top, margins.right, margins.bottom, margins.left);
            }

            let renderer = renderers_for_surfaces.get_mut(surface_id).unwrap();

            renderer.set_buffer_scale(surface.get_buffer_scale());
            renderer.clear(None);

            render(renderer, notification, output_spec);
            surface.write(renderer.get_backend());
        }
    };

    if !offset_per_output.contains_key(&output_name) {
        offset_per_output.insert(output_name.clone(), 0);
    }
    let offset = offset_per_output.get_mut(&output_name).unwrap();

    with_offset(offset, output_spec, try_rendering);

    return SurfaceProcessingOutput::Continue;
}

fn main() {
    let conn = Connection::connect_to_env().unwrap();

    let display = conn.display();

    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let _registry = display.get_registry(&qh, ());

    let mut global_data = backend::GlobalData::new(qh);
    event_queue.roundtrip(&mut global_data.get_mut()).unwrap();

    let mut socket_handler = backend::SocketManager::new(&mut event_queue, &mut global_data);
    while global_data.get_mut().outputs.len() == 0 {
        socket_handler.handle(10);
    }

    let mut notification_manager = NotificationManager::new();

    let width: usize = 300;
    let height: usize = 150;

    let mut renderers_for_surfaces = HashMap::<backend::ObjectId, Renderer>::new();

    let mut notification = Notification::new(
        "New notification".to_owned(),
        "This is a new notification from bell from second 0!".to_owned(),
    );
    for output_name in global_data.get_mut().outputs.keys() {
        let mut specifier = OutputSpecifier::new(width as i32, height as i32);
        specifier.set_anchor(backend::Anchor::Right | backend::Anchor::Bottom);
        specifier.set_growth_direction(notification::GrowthDirection::Left);
        specifier.set_layer(backend::Layer::Overlay);
        specifier.set_margins(0, 10, 0, 10);

        specifier.border_color = Some(Color::rgba(0xFF, 0x40, 0x40, 0xFF));
        specifier.border_size = 2;

        notification.add_output(output_name, specifier);
    }
    notification.create_surfaces(global_data.get_mut());

    notification_manager.add_notification(notification);

    let time_now = time::Instant::now();
    let mut created_notification = false;
    while notification_manager.is_active() {
        let notification = notification_manager.get_by_index(0).unwrap();
        notification.set_message( format!("This is a new notification from bell from second {}!", time_now.elapsed().as_secs() ));

        if time_now.elapsed().as_secs() >= 2 && !created_notification {
            let mut new_notification = Notification::new(
                "New new notification".to_owned(),
                "This is a newer new notification from bell!".to_owned(),
            );
            for output_name in global_data.get_mut().outputs.keys() {
                let mut specifier = OutputSpecifier::new(width as i32, height as i32);
                specifier.set_anchor(backend::Anchor::Right | backend::Anchor::Bottom);
                specifier.set_growth_direction(notification::GrowthDirection::Left);
                specifier.set_layer(backend::Layer::Overlay);
                specifier.set_margins(0, 10, 0, 10);

                specifier.border_color = Some(Color::rgba(0x40, 0x40, 0xFF, 0xFF));
                specifier.border_size = 2;

                new_notification.add_output(output_name, specifier);
            }
            new_notification.create_surfaces(global_data.get_mut());

            notification_manager.add_notification(new_notification);

            created_notification = true;
        }

        if time_now.elapsed().as_secs() >= 5 {
            notification_manager.remove_by_index(1);
            notification_manager.remove_by_index(0);
        }

        let manager_callback = |surface_id: &backend::ObjectId, notification: &Notification, offset_per_output: &mut HashMap<String, i32>| {
            process_surface(&mut global_data, &mut renderers_for_surfaces, offset_per_output, notification, surface_id)
        };

        notification_manager.process_active_notifications(manager_callback);

        socket_handler.handle(50);
    }
}
