pub use cosmic_text::{Attrs, Color, Metrics};

#[macro_export]
macro_rules! with_scale {
    ( $self:expr,$x:expr ) => { ($x as usize) * $self.buffer_scale };
    ( $self:expr,$( $x:expr ),+ ) => { ($( ($x as usize) * $self.buffer_scale, )+ )};
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
    use std::fmt;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    use png::{ColorType, OutputInfo};

    use crate::render::{blend_colors, text::*};

    const PI_2: f32 = 2.0 * std::f32::consts::PI;

    #[derive(Debug)]
    pub struct DrawPNGError<'a>(String, &'a OutputInfo);

    impl fmt::Display for DrawPNGError<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{} - {:?}", self.0, self.1)
        }
    }

    pub struct Renderer {
        backing_store: Vec<u32>,
        backing_store_stride: usize,

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
                backing_store_stride: width,
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

            self.backing_store_stride = self.width * self.buffer_scale;
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
            max_width: usize,
            max_height: usize,
            default_options: Attrs<'a>,
        ) {
            self.text_renderer.draw_text_spans(
                &mut self.backing_store,
                text_spans,
                x,
                y,
                max_width,
                max_height,
                default_options,
            );
        }

        pub fn draw_image(
            &mut self,
            x: i32,
            y: i32,
            width: usize,
            height: usize,
            image: &crate::dbus::ImageData,
        ) {
            let (x, y) = self.wrap_position(x, y);

            let width_scale = (width as f32) / (image.width as f32);
            let height_scale = (height as f32) / (image.height as f32);

            for y_offset in 0..image.height {
                let y_point = y + (y_offset as f32 * height_scale) as usize;
                for x_offset in 0..image.width {
                    let x_point = x + (x_offset as f32 * width_scale) as usize;

                    let base_image_offset =
                        (y_offset * image.rowstride + x_offset * image.channels) as usize;
                    let r = image.data[base_image_offset];
                    let g = image.data[base_image_offset + 1];
                    let b = image.data[base_image_offset + 2];
                    let a = if image.has_alpha {
                        image.data[base_image_offset + 3]
                    } else {
                        0xFF
                    };

                    let color = Color::rgba(r, g, b, a);

                    self.draw_point_with_scale(
                        x_point,
                        y_point,
                        Some(width_scale),
                        Some(height_scale),
                        color,
                    );
                }
            }
        }

        pub fn draw_png(
            &mut self,
            x: i32,
            y: i32,
            width: usize,
            height: usize,
            path: &PathBuf,
        ) -> Result<(), Error> {
            let (x, y) = self.wrap_position(x, y);

            use std::fs::File;
            use std::io::BufReader;
            let reader = BufReader::new(File::open(path)?);

            use png::{Decoder, Limits};
            let limits = Limits {
                bytes: width * height * 10,
            }; // Allow at most 80 bits/pixel (max 64 bits/pixel + metadata).
            let decoder = Decoder::new_with_limits(reader, limits);

            let mut image_reader = decoder.read_info()?;

            let required_buffer_size = image_reader
                .output_buffer_size()
                .ok_or(Error::from(ErrorKind::FileTooLarge))?;
            let mut frame_data_buffer = vec![0u8; required_buffer_size];

            let frame_information = image_reader.next_frame(&mut frame_data_buffer)?;

            Ok(self
                .draw_png_frame_to_buffer(
                    x,
                    y,
                    width,
                    height,
                    &frame_data_buffer,
                    &frame_information,
                )
                .unwrap())
        }

        fn draw_png_frame_to_buffer<'a>(
            &mut self,
            x: usize,
            y: usize,
            requested_width: usize,
            requested_height: usize,
            frame_data_buffer: &[u8],
            frame_information: &'a OutputInfo,
        ) -> Result<(), DrawPNGError<'a>> {
            let buffer_point_to_pixel: Box<fn(u64, u8, u64) -> Color>;

            match frame_information.color_type {
                ColorType::Grayscale => {
                    fn convert(raw_pixel_data: u64, _: u8, sample_bit_mask: u64) -> Color {
                        let value = (raw_pixel_data & sample_bit_mask) as u8;

                        Color::rgba(value, value, value, 0xFF)
                    }

                    buffer_point_to_pixel = Box::new(convert);
                }
                ColorType::Rgb => {
                    fn convert(
                        raw_pixel_data: u64,
                        sample_bit_depth: u8,
                        sample_bit_mask: u64,
                    ) -> Color {
                        let r = ((raw_pixel_data & (sample_bit_mask << 0 * sample_bit_depth))
                            >> 0 * sample_bit_depth) as u8;
                        let g = ((raw_pixel_data & (sample_bit_mask << 1 * sample_bit_depth))
                            >> 1 * sample_bit_depth) as u8;
                        let b = ((raw_pixel_data & (sample_bit_mask << 2 * sample_bit_depth))
                            >> 2 * sample_bit_depth) as u8;

                        Color::rgba(r, g, b, 0xFF)
                    }

                    buffer_point_to_pixel = Box::new(convert);
                }
                ColorType::Indexed => {
                    todo!()
                }
                ColorType::GrayscaleAlpha => {
                    fn convert(
                        raw_pixel_data: u64,
                        sample_bit_depth: u8,
                        sample_bit_mask: u64,
                    ) -> Color {
                        let value = ((raw_pixel_data & (sample_bit_mask << 0 * sample_bit_depth))
                            >> 0 * sample_bit_depth) as u8;
                        let alpha = ((raw_pixel_data & (sample_bit_mask << 1 * sample_bit_depth))
                            >> 1 * sample_bit_depth) as u8;

                        Color::rgba(value, value, value, alpha)
                    }

                    buffer_point_to_pixel = Box::new(convert);
                }
                ColorType::Rgba => {
                    fn convert(
                        raw_pixel_data: u64,
                        sample_bit_depth: u8,
                        sample_bit_mask: u64,
                    ) -> Color {
                        let r = ((raw_pixel_data & (sample_bit_mask << 0 * sample_bit_depth))
                            >> 0 * sample_bit_depth) as u8;
                        let g = ((raw_pixel_data & (sample_bit_mask << 1 * sample_bit_depth))
                            >> 1 * sample_bit_depth) as u8;
                        let b = ((raw_pixel_data & (sample_bit_mask << 2 * sample_bit_depth))
                            >> 2 * sample_bit_depth) as u8;
                        let a = ((raw_pixel_data & (sample_bit_mask << 3 * sample_bit_depth))
                            >> 3 * sample_bit_depth) as u8;

                        Color::rgba(r, g, b, a)
                    }

                    buffer_point_to_pixel = Box::new(convert);
                }
            }

            let sample_bit_depth = frame_information.bit_depth as u8;
            let mut sample_bit_mask = u64::from((1u16 << sample_bit_depth) - 1);

            if sample_bit_depth > 8 {
                eprintln!(
                    "Trying to decode a PNG file with {} bits/sample. Currently we can only support up to 8 bits/sample.",
                    sample_bit_depth
                );
                eprintln!("Image bit values will be truncated to try displaying something at all.");

                sample_bit_mask = 0x00000000_FFFFFFFF;
            }

            let bits_per_pixel =
                frame_information.color_type.samples() * usize::from(sample_bit_depth);
            assert!(bits_per_pixel <= 64);

            let raw_value_from_offsets = |byte_offset: usize, bit_offset: usize| {
                let mut value: u64 = 0;
                let mut bits_parsed: i32 = -(bit_offset as i32);

                let mut iteration = 0;
                while bits_parsed < (bits_per_pixel as i32) {
                    if byte_offset + iteration >= frame_data_buffer.len() {
                        return None;
                    }
                    let partial = frame_data_buffer[byte_offset + iteration] as u64;
                    value |= partial << 8 * iteration;

                    bits_parsed += 8;
                    iteration += 1;
                }

                // FIXME: Properly parse last 'bit_offset' bits when handling 64 bits/pixel data.
                return Some(value >> bit_offset);
            };

            let buffer_width = frame_information.width as usize;
            let buffer_height = frame_information.height as usize;

            let x_scaling = (requested_width as f32) / (buffer_width as f32);
            let y_scaling = (requested_height as f32) / (buffer_height as f32);

            let frame_stride_bits = frame_information.line_size * 8;
            for y_point in 0..buffer_height {
                for x_point in 0..buffer_width {
                    let buffer_offset_bits = y_point * frame_stride_bits + x_point * bits_per_pixel;

                    let current_byte_offset = buffer_offset_bits.div_euclid(8);
                    let current_bit_offset = buffer_offset_bits.rem_euclid(8);

                    let raw_pixel_data =
                        raw_value_from_offsets(current_byte_offset, current_bit_offset)
                            .ok_or_else(|| DrawPNGError(format!("Failed to construct raw pixel data @ (x={} y={}) from bit offsets. Offsets: {} bytes / {} bits / {} total buffer size", x_point, y_point, current_byte_offset, current_bit_offset, frame_data_buffer.len()), &frame_information))?;
                    let pixel_color =
                        buffer_point_to_pixel(raw_pixel_data, sample_bit_depth, sample_bit_mask);

                    self.draw_point_with_scale(
                        x + (x_scaling * (x_point as f32)) as usize,
                        y + (y_scaling * (y_point as f32)) as usize,
                        Some(x_scaling),
                        Some(y_scaling),
                        pixel_color,
                    );
                }
            }

            Ok(())
        }

        pub fn draw_border(&mut self, size: usize, radius: Option<usize>, color: Color) {
            let width = self.width;
            let height = self.height;

            let radius = radius.unwrap_or(0);
            let radius_i = radius as i32;

            // Top
            self.draw_rect(radius_i, 0, width - 2 * radius, size, color);
            // Bottom
            self.draw_rect(radius_i, -(size as i32), width - 2 * radius, size, color);

            // Left
            self.draw_rect(0, radius_i, size, height - 2 * radius, color);
            // Right
            self.draw_rect(-(size as i32), radius_i, size, height - 2 * radius, color);

            if radius != 0 {
                use std::f32::consts as Constant;

                let transparent = Some(Color::rgba(0, 0, 0, 0));

                self.draw_arc(
                    radius_i,
                    radius_i,
                    radius,
                    Constant::FRAC_PI_2,
                    Constant::PI,
                    size as f32,
                    self.clear_color,
                    color,
                    transparent,
                );
                self.draw_arc(
                    -radius_i,
                    radius_i,
                    radius,
                    0.0,
                    Constant::FRAC_PI_2,
                    size as f32,
                    self.clear_color,
                    color,
                    transparent,
                );
                self.draw_arc(
                    radius_i,
                    -radius_i,
                    radius,
                    Constant::PI,
                    3.0 * Constant::FRAC_PI_2,
                    size as f32,
                    self.clear_color,
                    color,
                    transparent,
                );
                self.draw_arc(
                    -radius_i,
                    -radius_i,
                    radius,
                    3.0 * Constant::FRAC_PI_2,
                    PI_2,
                    size as f32,
                    self.clear_color,
                    color,
                    transparent,
                );
            }
        }

        pub fn draw_arc(
            &mut self,
            center_x: i32,
            center_y: i32,
            radius: usize,
            start_angle: f32,
            end_angle: f32,
            border_size: f32,
            fill_color: Color,
            border_color: Color,
            outside_color: Option<Color>,
        ) {
            let radius = radius as i32;
            let radius_sq = radius.saturating_pow(2);

            for y in (-radius)..(radius + 1) {
                for x in (-radius)..(radius + 1) {
                    // x = r * cos(th) | y = - r * sin(th) => tan(th) = - y / x => th = atan(- y / x)
                    let angle = (f32::atan2(-y as f32, x as f32) + PI_2).rem_euclid(PI_2);

                    // 1e-3 is here to provide some leeway for floating-point rounding errors.
                    if angle < start_angle - 1e-3 || angle > end_angle + 1e-3 {
                        // Handle 'end_angle == 2pi' scenario, since it wraps back to 0.
                        if end_angle < PI_2 || angle > (end_angle.rem_euclid(PI_2) + 1e-3) {
                            continue;
                        }
                    }

                    let (x_point, y_point) = self.wrap_position(x + center_x, y + center_y);

                    let distance_sq = x.saturating_pow(2) + y.saturating_pow(2);
                    if distance_sq >= radius_sq {
                        if let Some(color) = outside_color {
                            // Outside the arc
                            self.draw_rect_with_scale(x_point, y_point, 1, 1, None, None, color, false);
                        }

                        continue;
                    }

                    if radius as f32 - (distance_sq as f32).sqrt() <= border_size {
                        // Close to the border
                        self.draw_point_with_scale(x_point, y_point, None, None, border_color);
                    } else {
                        // Inside the arc
                        self.draw_point_with_scale(x_point, y_point, None, None, fill_color);
                    }
                }
            }
        }

        pub fn draw_rect(&mut self, x: i32, y: i32, width: usize, height: usize, color: Color) {
            let (x, y) = self.wrap_position(x, y);

            self.draw_rect_with_scale(x, y, width, height, None, None, color, true);
        }

        pub fn clear(&mut self, color: Option<Color>) {
            let color = color.unwrap_or(self.clear_color);
            self.backing_store.fill(color.0);
        }

        fn draw_point_with_scale(
            &mut self,
            x_original: usize,
            y_original: usize,
            width_scale: Option<f32>,
            height_scale: Option<f32>,
            color: Color,
        ) {
            self.draw_rect_with_scale(
                x_original,
                y_original,
                1,
                1,
                width_scale,
                height_scale,
                color,
                true,
            );
        }

        fn draw_rect_with_scale(
            &mut self,
            x_original: usize,
            y_original: usize,
            width_original: usize,
            height_original: usize,
            width_scale: Option<f32>,
            height_scale: Option<f32>,
            mut color: Color,
            blend_with_previous_color: bool,
        ) {
            let width_scale = width_scale.unwrap_or(1.0);
            let height_scale = height_scale.unwrap_or(1.0);

            let scale_end =
                |scale: f32, original: usize| (scale * f32::from(original as u16)).ceil() as usize;

            let y_start = self.buffer_scale * y_original;
            let y_end = self.buffer_scale
                * y_original.saturating_add(scale_end(height_scale, height_original));
            for y in y_start..y_end {
                let x_start = self.buffer_scale * x_original;
                let x_end = self.buffer_scale
                    * x_original.saturating_add(scale_end(width_scale, width_original));
                for x in x_start..x_end {
                    let offset = y * self.backing_store_stride + x;
                    if blend_with_previous_color && color.a() != 0xFF {
                        color = blend_colors(Color(self.backing_store[offset]), color)
                    }

                    assert!(offset < self.backing_store.len());
                    self.backing_store[offset] = color.0;
                }
            }
        }

        const fn wrap_position(&self, x: i32, y: i32) -> (usize, usize) {
            let x = if x >= 0 { x } else { (self.width as i32) + x };
            let y = if y >= 0 { y } else { (self.height as i32) + y };

            (x as usize, y as usize)
        }
    }
}

pub mod text {
    pub use crate::render::{Attrs, Color, Metrics};
    use cosmic_text::{Align, Buffer, FontSystem, Shaping, SwashCache};

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

        pub fn draw_text_spans<'a>(
            &mut self,
            backend: &mut [u32],
            mut text_spans: Vec<(&str, Attrs<'a>)>,
            x: i32,
            y: i32,
            max_width: usize,
            max_height: usize,
            default_options: Attrs<'a>,
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

            let (x, y) = with_scale!(self, x, y);
            let (width, max_width, height, max_height) =
                with_scale!(self, self.width, max_width, self.height, max_height);
            let (buffer_width, buffer_height) = (
                f32::from(usize::min(width - x, max_width) as u16),
                f32::from(usize::min(height - y, max_height) as u16),
            );
            buffer.set_size(Some(buffer_width), Some(buffer_height));

            buffer.set_rich_text(str_spans, &default_options, Shaping::Advanced, Some(Align::Left));

            use cosmic_text::Wrap;
            buffer.set_wrap(Wrap::WordOrGlyph);

            // FIXME: This option seems to only take into account the metrics of each text span individually,
            // so if e.g. the body is split into two spans, it can have like two lines per span and still not
            // be ellipsized.
            use cosmic_text::{Ellipsize, EllipsizeHeightLimit};
            buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(2)));

            let mut callback = |x_glyph, y_glyph, w, h, c| {
                TextRenderer::draw_callback(
                    backend,
                    with_scale!(self, self.width),
                    (x + x_glyph as usize) as u32,
                    (y + y_glyph as usize) as u32,
                    w,
                    h,
                    c,
                )
            };

            buffer.draw(
                &mut self.swash_cache,
                Color::rgb(0x00, 0x00, 0x00),
                &mut callback,
            );
        }

        fn draw_callback(
            backend: &mut [u32],
            backend_stride: usize,
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
                    let index = ((y + idy) * (backend_stride as u32) + (x + idx)) as usize;
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
