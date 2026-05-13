/// GPU renderer backed by `vello` + `wgpu`.
///
/// Gated behind `--features gpu`. Build and test with:
///   cargo build --features gpu
///   cargo test  --features gpu
///
/// Headless mode (no window): uses wgpu without a surface; reads back RGBA pixels
/// via a staging buffer after each frame.  Requires a GPU (or LLVM software rasterizer
/// via wgpu's dx12/vulkan WARP/lavapipe backend on CI).

#[cfg(feature = "gpu")]
mod inner {
    use std::num::NonZeroUsize;
    use std::sync::mpsc;

    use pollster::block_on;
    use wgpu;

    use vello::{
        AaConfig, AaSupport, RenderParams,
        Renderer as GpuRenderer, RendererOptions, Scene as VelloScene,
    };
    use vello::peniko::{
        self, BlendMode, Brush, Color as PenikoColor, Fill as PenikoFill,
        Gradient as PenikoGradient, ColorStop,
    };
    use vello::kurbo::{Affine, BezPath, PathEl, Point as KurboPoint, Stroke as KurboStroke};

    use nalgebra::Matrix3;

    use crate::effects::Effect;
    use crate::paint::{BlendMode as AnimBlendMode, Color, Fill, Gradient, GradientStop, Paint};
    use crate::path::{AnimPath, PathVerb};
    use crate::renderer::Renderer;

    // ── GPU context ───────────────────────────────────────────────────────────

    struct GpuCtx {
        device:   wgpu::Device,
        queue:    wgpu::Queue,
        renderer: GpuRenderer,
    }

    pub struct VelloRenderer {
        width:      u32,
        height:     u32,
        background: [u8; 4],
        scene:      VelloScene,
        ctx:        Option<GpuCtx>,
    }

    impl VelloRenderer {
        /// Create a headless GPU renderer. Falls back gracefully if no adapter is found.
        pub fn new_headless() -> Self {
            let ctx = block_on(init_wgpu());
            Self { width: 0, height: 0, background: [255, 255, 255, 255], scene: VelloScene::new(), ctx }
        }

        /// Returns `true` when a GPU context was successfully initialised.
        pub fn is_gpu_available(&self) -> bool {
            self.ctx.is_some()
        }
    }

    async fn init_wgpu() -> Option<GpuCtx> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: true, // allow software rasterizer on CI
            })
            .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    label: Some("animcore"),
                },
                None,
            )
            .await
            .ok()?;

        let renderer = GpuRenderer::new(
            &device,
            RendererOptions {
                surface_format: None,
                use_cpu: false,
                antialiasing_support: AaSupport::all(),
                num_init_threads: NonZeroUsize::new(1),
            },
        )
        .ok()?;

        Some(GpuCtx { device, queue, renderer })
    }

    // ── Renderer impl ─────────────────────────────────────────────────────────

    impl Renderer for VelloRenderer {
        fn begin_frame(&mut self, width: u32, height: u32, background: [u8; 4]) {
            self.width      = width;
            self.height     = height;
            self.background = background;
            self.scene      = VelloScene::new();
        }

        fn draw_path(
            &mut self,
            path:      &AnimPath,
            paint:     &Paint,
            transform: &Matrix3<f32>,
            opacity:   f32,
            _effects:  &[Effect], // TODO: map to vello layer + blur
        ) {
            let affine = mat3_to_affine(transform);
            let bp     = anim_path_to_bez(path);
            let alpha  = (paint.opacity * opacity).clamp(0.0, 1.0);

            // Fill
            if let Some(brush) = fill_to_brush(&paint.fill, alpha) {
                self.scene.fill(PenikoFill::NonZero, affine, &brush, None, &bp);
            }

            // Stroke
            if let Some(stroke_cfg) = &paint.stroke {
                if let Some(brush) = fill_to_brush(&stroke_cfg.fill, alpha) {
                    let sk = KurboStroke::new(stroke_cfg.width as f64);
                    self.scene.stroke(&sk, affine, &brush, None, &bp);
                }
            }
        }

        fn push_clip(&mut self, path: &AnimPath, transform: &Matrix3<f32>) {
            let affine = mat3_to_affine(transform);
            let bp     = anim_path_to_bez(path);
            // peniko::Mix::Clip creates a clipping layer in vello.
            self.scene.push_layer(peniko::Mix::Clip, 1.0, affine, &bp);
        }

        fn pop_clip(&mut self) {
            self.scene.pop_layer();
        }

        fn draw_pixels(
            &mut self,
            pixels: &[u8],
            width:  u32,
            height: u32,
            transform: &Matrix3<f32>,
            opacity:   f32,
        ) {
            let data  = peniko::Blob::new(std::sync::Arc::new(pixels.to_vec()));
            let image = peniko::Image::new(data, peniko::Format::Rgba8, width, height);
            let affine = mat3_to_affine(transform);
            // Push an opacity layer and blit the image.
            let bounds = vello::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64);
            self.scene.push_layer(peniko::Mix::Normal, opacity, affine, &bounds);
            self.scene.draw_image(&image, Affine::IDENTITY);
            self.scene.pop_layer();
        }

        fn end_frame(&mut self) -> Vec<u8> {
            let ctx = match &mut self.ctx {
                Some(c) => c,
                None => return vec![0u8; (self.width * self.height * 4) as usize],
            };

            let [br, bg, bb, _] = self.background;
            let bg_color = PenikoColor::from_rgba8(br, bg, bb, 255);

            // Create an output texture (RGBA8, storage + copy-src).
            let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
                label:             Some("animcore_out"),
                size:              wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
                mip_level_count:   1,
                sample_count:      1,
                dimension:         wgpu::TextureDimension::D2,
                format:            wgpu::TextureFormat::Rgba8Unorm,
                usage:             wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats:      &[],
            });
            let view = texture.create_view(&Default::default());

            block_on(ctx.renderer.render_to_texture(
                &ctx.device,
                &ctx.queue,
                &self.scene,
                &view,
                &RenderParams {
                    base_color:          bg_color,
                    width:               self.width,
                    height:              self.height,
                    antialiasing_method: AaConfig::Msaa16,
                },
            )).ok();

            // Copy texture → staging buffer.
            let bytes_per_row = align256(self.width * 4);
            let buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label:              Some("animcore_readback"),
                size:               (bytes_per_row * self.height) as u64,
                usage:              wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            let mut enc = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("animcore_copy"),
            });
            enc.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture:  &texture,
                    mip_level: 0,
                    origin:   wgpu::Origin3d::ZERO,
                    aspect:   wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &buf,
                    layout: wgpu::ImageDataLayout {
                        offset:         0,
                        bytes_per_row:  Some(bytes_per_row),
                        rows_per_image: Some(self.height),
                    },
                },
                wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
            );
            ctx.queue.submit([enc.finish()]);

            let slice = buf.slice(..);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
            ctx.device.poll(wgpu::Maintain::Wait);
            let _ = rx.recv();

            let mapped = slice.get_mapped_range();
            let mut out = vec![0u8; (self.width * self.height * 4) as usize];
            for row in 0..self.height as usize {
                let src = row * bytes_per_row as usize;
                let dst = row * self.width as usize * 4;
                out[dst..dst + self.width as usize * 4]
                    .copy_from_slice(&mapped[src..src + self.width as usize * 4]);
            }
            out
        }
    }

    // ── Conversion helpers ────────────────────────────────────────────────────

    fn mat3_to_affine(m: &Matrix3<f32>) -> Affine {
        Affine::new([
            m[(0, 0)] as f64, m[(1, 0)] as f64,
            m[(0, 1)] as f64, m[(1, 1)] as f64,
            m[(0, 2)] as f64, m[(1, 2)] as f64,
        ])
    }

    fn anim_path_to_bez(path: &AnimPath) -> BezPath {
        let mut bp = BezPath::new();
        let mut pi = 0usize;
        for verb in &path.verbs {
            match verb {
                PathVerb::MoveTo => {
                    let [x, y] = path.points[pi]; pi += 1;
                    bp.push(PathEl::MoveTo(KurboPoint::new(x as f64, y as f64)));
                }
                PathVerb::LineTo => {
                    let [x, y] = path.points[pi]; pi += 1;
                    bp.push(PathEl::LineTo(KurboPoint::new(x as f64, y as f64)));
                }
                PathVerb::CubicTo => {
                    let [cx1, cy1] = path.points[pi];
                    let [cx2, cy2] = path.points[pi + 1];
                    let [x,   y  ] = path.points[pi + 2];
                    pi += 3;
                    bp.push(PathEl::CurveTo(
                        KurboPoint::new(cx1 as f64, cy1 as f64),
                        KurboPoint::new(cx2 as f64, cy2 as f64),
                        KurboPoint::new(x   as f64, y   as f64),
                    ));
                }
                PathVerb::QuadTo => {
                    let [cx, cy] = path.points[pi];
                    let [x,  y ] = path.points[pi + 1];
                    pi += 2;
                    bp.push(PathEl::QuadTo(
                        KurboPoint::new(cx as f64, cy as f64),
                        KurboPoint::new(x  as f64, y  as f64),
                    ));
                }
                PathVerb::Close => { bp.push(PathEl::ClosePath); }
            }
        }
        bp
    }

    fn color_to_peniko(c: Color, opacity: f32) -> PenikoColor {
        PenikoColor::from_rgba8(
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            ((c.a * opacity) * 255.0) as u8,
        )
    }

    fn fill_to_brush(fill: &Fill, opacity: f32) -> Option<Brush> {
        match fill {
            Fill::None => None,
            Fill::Solid(c) => Some(Brush::Solid(color_to_peniko(*c, opacity))),
            Fill::Gradient(g) => Some(gradient_to_brush(g, opacity)),
        }
    }

    fn gradient_to_brush(grad: &Gradient, opacity: f32) -> Brush {
        match grad {
            Gradient::Linear { start, end, stops } => {
                let mut g = PenikoGradient::new_linear(
                    KurboPoint::new(start[0] as f64, start[1] as f64),
                    KurboPoint::new(end[0]   as f64, end[1]   as f64),
                );
                for s in stops {
                    g = g.with_stop(ColorStop {
                        offset: s.position,
                        color:  color_to_peniko(s.color, opacity),
                    });
                }
                Brush::Gradient(g)
            }
            Gradient::Radial { center, radius, stops } => {
                let c = KurboPoint::new(center[0] as f64, center[1] as f64);
                let mut g = PenikoGradient::new_radial(c, *radius as f64);
                for s in stops {
                    g = g.with_stop(ColorStop {
                        offset: s.position,
                        color:  color_to_peniko(s.color, opacity),
                    });
                }
                Brush::Gradient(g)
            }
        }
    }

    /// Row stride must be a multiple of 256 bytes for wgpu buffer copies.
    fn align256(x: u32) -> u32 {
        (x + 255) & !255
    }
}

#[cfg(feature = "gpu")]
pub use inner::VelloRenderer;
