pub use cosmic_text::{Attrs, Color, Metrics};

#[macro_export]
macro_rules! with_scale {
    ( $self:expr,$x:expr ) => {{ ($x as usize) * $self.buffer_scale }};
}

fn blend_colors(first_color: Color, second_color: Color) -> Color {
    let a_first = f32::from(first_color.a());
    let a_second = f32::from(second_color.a());

    let first_frac = a_first / (a_first + a_second);
    let second_frac = a_second / (a_first + a_second);

    let r = unsafe {
        (first_frac * f32::from(first_color.r()) + second_frac * f32::from(second_color.r()))
            .to_int_unchecked::<u32>()
    };
    let g = unsafe {
        (first_frac * f32::from(first_color.g()) + second_frac * f32::from(second_color.g()))
            .to_int_unchecked::<u32>()
    };
    let b = unsafe {
        (first_frac * f32::from(first_color.b()) + second_frac * f32::from(second_color.b()))
            .to_int_unchecked::<u32>()
    };
    let a = unsafe {
        (first_frac * f32::from(first_color.a()) + second_frac * f32::from(second_color.a()))
            .to_int_unchecked::<u32>()
    };

    let color_bits = ((a << 24) & 0xFF000000)
        + ((r << 16) & 0x00FF0000)
        + ((g << 8) & 0x0000FF00)
        + ((b << 0) & 0x000000FF);
    return Color(color_bits);
}

pub mod render {
    use crate::render::text::*;

    pub struct Renderer {
        backing_store: Vec<u32>,

        pub width: usize,
        pub height: usize,
        buffer_scale: usize,

        text_renderer: TextRenderer,

        clear_color: Color,
    }

    impl Renderer {
        pub fn new(width: usize, height: usize, clear_color: Color) -> Renderer {
            let text_renderer = TextRenderer::new(width, height, 1);

            Renderer {
                backing_store: vec![0; width * height],
                width,
                height,
                buffer_scale: 1,
                text_renderer,
                clear_color,
            }
        }

        pub fn get_backing_store(&mut self) -> &mut [u32] {
            self.backing_store.as_mut_slice()
        }

        pub fn set_width(&mut self, width: usize) {
            self.width = width;
            self.text_renderer.width = width;

            self.backing_store.resize(
                width * self.height * (self.buffer_scale.pow(2)) as usize,
                self.clear_color.0,
            );
        }

        pub fn set_height(&mut self, height: usize) {
            self.height = height;
            self.text_renderer.height = height;

            self.backing_store.resize(
                self.width * height * (self.buffer_scale.pow(2)) as usize,
                self.clear_color.0,
            );
        }

        pub fn set_buffer_scale(&mut self, scale_factor: i32) {
            if (scale_factor as usize) == self.buffer_scale {
                return;
            }

            self.buffer_scale = scale_factor as usize;
            self.text_renderer.buffer_scale = self.buffer_scale;

            self.backing_store.resize(
                self.width * self.height * (self.buffer_scale.pow(2)) as usize,
                self.clear_color.0,
            );
        }

        /// Draws a string of text to the backing store.
        ///
        /// The x and y coordinates are surface-local coordinates representing the
        /// top-left corner from which text rendering will start.
        pub fn draw_text(
            &mut self,
            text: &String,
            x: i32,
            y: i32,
            color: Color,
            options: TextRenderOptions,
        ) {
            self.text_renderer
                .draw_text(&mut self.backing_store, text, x, y, color, options);
        }

        /// Draws a series of strings of text to the backing store with per-item customization.
        ///
        /// The x and y coordinates are surface-local coordinates representing the
        /// top-left corner from which text rendering will start.
        pub fn draw_text_spans<'a>(
            &mut self,
            text_spans: Vec<(&str, Attrs<'a>)>,
            x: i32,
            y: i32,
            options: TextRenderOptions,
        ) {
            self.text_renderer
                .draw_text_spans(&mut self.backing_store, text_spans, x, y, options);
        }

        pub fn draw_rect(&mut self, x: i32, y: i32, width: usize, height: usize, color: Color) {
            let x = with_scale!(self, x as usize);
            let y = with_scale!(self, y as usize);

            let width = with_scale!(self, width);
            let height = with_scale!(self, height);

            let backend_width = with_scale!(self, self.width);

            let x_end = x.saturating_add(width);
            assert!(x_end <= backend_width);
            let y_end = y.saturating_add(height);
            assert!(y_end <= with_scale!(self, self.height));

            for y_point in y..y_end {
                for x_point in x..x_end {
                    self.backing_store[y_point * backend_width + x_point] = color.0;
                }
            }
        }

        pub fn draw_border(&mut self, size: usize, color: Color) {
            let width = self.width;
            let height = self.height;

            // Top
            self.draw_rect(0, 0, width, size, color);
            // Bottom
            self.draw_rect(
                size as i32,
                (self.height - size) as i32,
                width - 2 * size,
                size,
                color,
            );

            // Left
            self.draw_rect(0, size as i32, size, height - 2 * size, color);
            // Right
            self.draw_rect(
                (self.width - size) as i32,
                size as i32,
                size,
                height - 2 * size,
                color,
            );
        }

        pub fn clear(&mut self, color: Option<Color>) {
            let color = color.unwrap_or(self.clear_color);
            self.backing_store.fill(color.0);
        }
    }
}

pub mod text {
    pub use crate::render::{Attrs, Color, Metrics};
    use cosmic_text::{Buffer, FontSystem, Shaping, SwashCache};

    pub struct TextRenderOptions<'a> {
        pub text_attributes: Attrs<'a>,

        /// Margin from the top or bottom of a glyph to the line boundary.
        pub line_height_margin: f32,
        /// Height in pixels of each glyph.
        pub font_size: f32,
    }

    impl TextRenderOptions<'_> {
        pub fn new() -> TextRenderOptions<'static> {
            TextRenderOptions {
                text_attributes: Attrs::new(),
                line_height_margin: 4.,
                font_size: 12.,
            }
        }
    }

    pub struct TextRenderer {
        pub width: usize,
        pub height: usize,
        pub buffer_scale: usize,

        font_system: FontSystem,
        swash_cache: SwashCache,

        buffer: Option<Buffer>,
    }

    impl TextRenderer {
        pub fn new(width: usize, height: usize, buffer_scale: usize) -> TextRenderer {
            let metrics = Metrics::new(12.0, 20.0);

            let mut renderer = TextRenderer {
                width,
                height,
                buffer_scale,
                font_system: FontSystem::new(),
                swash_cache: SwashCache::new(),
                buffer: None,
            };

            renderer
                .buffer
                .replace(Buffer::new(&mut renderer.font_system, metrics));

            renderer
        }

        pub fn draw_text(
            &mut self,
            backend: &mut [u32],
            text: &String,
            x: i32,
            y: i32,
            color: Color,
            options: TextRenderOptions,
        ) {
            let metrics = Metrics::new(
                options.font_size,
                options.font_size + 2.0 * options.line_height_margin,
            );
            let metrics = self.scale_metrics(metrics);

            let buffer = self.buffer.as_mut().unwrap();
            let current_metrics = buffer.metrics();

            if current_metrics != metrics {
                buffer.set_metrics(metrics);
            }

            let mut buffer = buffer.borrow_with(&mut self.font_system);
            buffer.set_size(
                Some(f32::from(self.width as u16 * self.buffer_scale as u16)),
                Some(f32::from(self.height as u16 * self.buffer_scale as u16)),
            );

            buffer.set_text(text, &options.text_attributes, Shaping::Advanced, None);

            let callback = |x_glyph, y_glyph, w, h, c| {
                TextRenderer::draw_callback(
                    backend,
                    with_scale!(self, self.width),
                    (x + x_glyph) as u32,
                    (y + y_glyph) as u32,
                    w,
                    h,
                    c,
                )
            };

            buffer.draw(&mut self.swash_cache, color, callback);
        }

        pub fn draw_text_spans<'a>(
            &mut self,
            backend: &mut [u32],
            mut text_spans: Vec<(&str, Attrs<'a>)>,
            x: i32,
            y: i32,
            default_options: TextRenderOptions,
        ) {
            let str_spans = {
                let attr_change = |attrs: &mut Attrs<'_>| {
                    attrs.metrics_opt = attrs
                        .metrics_opt
                        .map(|cache_metrics| self.scale_metrics(cache_metrics.into()).into())
                };
                text_spans
                    .iter_mut()
                    .for_each(|(_, attrs)| attr_change(attrs));
                text_spans.into_iter()
            };

            let mut buffer = self
                .buffer
                .as_mut()
                .unwrap()
                .borrow_with(&mut self.font_system);
            buffer.set_rich_text(
                str_spans,
                &default_options.text_attributes,
                Shaping::Advanced,
                None,
            );

            let callback = |x_glyph, y_glyph, w, h, c| {
                TextRenderer::draw_callback(
                    backend,
                    with_scale!(self, self.width),
                    (x + x_glyph) as u32,
                    (y + y_glyph) as u32,
                    w,
                    h,
                    c,
                )
            };

            buffer.draw(
                &mut self.swash_cache,
                Color::rgb(0x00, 0x00, 0x00),
                callback,
            );
        }

        fn draw_callback(
            backend: &mut [u32],
            width: usize,
            x: u32,
            y: u32,
            glyph_width: u32,
            glyph_height: u32,
            color: Color,
        ) {
            let buffer_size = backend.len();

            for idy in 0..glyph_height {
                for idx in 0..glyph_width {
                    // NOTE: We let it overflow naturally, as we check it against buffer_size right after.
                    let index = ((y + idy) * (width as u32) + (x + idx)) as usize;
                    if index >= buffer_size {
                        continue;
                    }

                    let mut actual_color = color;
                    if color.a() != 0xFF {
                        actual_color = crate::render::blend_colors(Color(backend[index]), color);
                    }

                    backend[index] = actual_color.0;
                }
            }
        }

        fn scale_metrics(&self, metrics: Metrics) -> Metrics {
            metrics.scale(f32::from(self.buffer_scale as u16))
        }
    }
}
