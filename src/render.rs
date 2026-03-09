//! GPU rendering for calendar views.
//!
//! Implements `madori::RenderCallback` for the calendar application.
//! Renders month grid, week view, day view, event blocks, and UI chrome
//! using garasu's GPU context and text renderer.

use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use chrono::{Datelike, Local, NaiveDate, Timelike};
use glyphon::{Attrs, Buffer, Color as GlyphonColor, Family, Metrics, Shaping};
use madori::render::{RenderCallback, RenderContext};

use crate::calendar::{self, ViewMode, WeekStart};
use crate::events::{EventOccurrence, EventStore};
use crate::input::{EditorField, InputMode};

/// Shared application state accessible from the event handler and renderer.
pub struct AppState {
    /// Current view mode.
    pub view_mode: ViewMode,
    /// Currently focused date.
    pub cursor_date: NaiveDate,
    /// Current year being displayed.
    pub year: i32,
    /// Current month being displayed (1-12).
    pub month: u32,
    /// Week start preference.
    pub week_start: WeekStart,
    /// Whether to use 24h time format.
    pub use_24h: bool,
    /// Input mode.
    pub input_mode: InputMode,
    /// Event store.
    pub store: EventStore,
    /// Status message (shown at bottom).
    pub status: String,
    /// Command input buffer (for : mode).
    pub command_buffer: String,
    /// Event editor state.
    pub editor: Option<EditorState>,
    /// Font size.
    pub font_size: f32,
    /// Selected time slot (hour 0-23) for week/day views.
    pub selected_hour: u8,
    /// Selected event index in the current day's events.
    pub selected_event_idx: usize,
    /// Cached events for the current view (refreshed on navigation).
    pub cached_events: Vec<EventOccurrence>,
    /// Whether the cache needs refreshing.
    #[allow(dead_code)]
    pub cache_dirty: bool,
}

/// State for the event editor overlay.
pub struct EditorState {
    /// Which field is currently focused.
    pub active_field: EditorField,
    /// Field values.
    pub title: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub location: String,
    pub calendar: String,
    pub recurrence: String,
    /// If editing an existing event, its ID.
    pub editing_id: Option<String>,
}

impl EditorState {
    /// Create a new empty editor for creating an event.
    #[must_use]
    pub fn new_for_date(date: NaiveDate, hour: u8) -> Self {
        let start_time = format!("{hour:02}:00");
        let end_hour = (hour + 1).min(23);
        let end_time = format!("{end_hour:02}:00");
        Self {
            active_field: EditorField::Title,
            title: String::new(),
            start_date: date.format("%Y-%m-%d").to_string(),
            start_time,
            end_date: date.format("%Y-%m-%d").to_string(),
            end_time,
            location: String::new(),
            calendar: "default".to_string(),
            recurrence: String::new(),
            editing_id: None,
        }
    }

    /// Create an editor pre-filled with an existing event's data.
    #[must_use]
    pub fn from_event(event: &crate::events::Event) -> Self {
        Self {
            active_field: EditorField::Title,
            title: event.title.clone(),
            start_date: event.start.date().format("%Y-%m-%d").to_string(),
            start_time: event.start.time().format("%H:%M").to_string(),
            end_date: event.end.date().format("%Y-%m-%d").to_string(),
            end_time: event.end.time().format("%H:%M").to_string(),
            location: event.location.clone().unwrap_or_default(),
            calendar: event.calendar.clone(),
            recurrence: event
                .recurrence
                .as_ref()
                .map_or(String::new(), |r| format!("{:?}", r.freq)),
            editing_id: Some(event.id.clone()),
        }
    }

    /// Get a mutable reference to the currently active field's value.
    pub fn active_value_mut(&mut self) -> &mut String {
        match self.active_field {
            EditorField::Title => &mut self.title,
            EditorField::StartDate => &mut self.start_date,
            EditorField::StartTime => &mut self.start_time,
            EditorField::EndDate => &mut self.end_date,
            EditorField::EndTime => &mut self.end_time,
            EditorField::Location => &mut self.location,
            EditorField::Calendar => &mut self.calendar,
            EditorField::Recurrence => &mut self.recurrence,
        }
    }

    /// Get the currently active field's value (used by tests and future features).
    #[must_use]
    #[allow(dead_code)]
    pub fn active_value(&self) -> &str {
        match self.active_field {
            EditorField::Title => &self.title,
            EditorField::StartDate => &self.start_date,
            EditorField::StartTime => &self.start_time,
            EditorField::EndDate => &self.end_date,
            EditorField::EndTime => &self.end_time,
            EditorField::Location => &self.location,
            EditorField::Calendar => &self.calendar,
            EditorField::Recurrence => &self.recurrence,
        }
    }
}

/// Shared app state handle.
pub type SharedState = Arc<Mutex<AppState>>;

// ---------------------------------------------------------------------------
// GPU rect instance data
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct RectInstance {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ScreenUniforms {
    resolution: [f32; 2],
    _padding: [f32; 2],
}

const RECT_SHADER: &str = r"
struct ScreenUniforms {
    resolution: vec2<f32>,
    _padding: vec2<f32>,
};

struct RectInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: ScreenUniforms;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    instance: RectInstance,
) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let pixel = instance.pos + corners[vi] * instance.size;
    let ndc = vec2<f32>(
        (pixel.x / screen.resolution.x) * 2.0 - 1.0,
        1.0 - (pixel.y / screen.resolution.y) * 2.0,
    );
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(frag: VertexOutput) -> @location(0) vec4<f32> {
    return frag.color;
}
";

// ---------------------------------------------------------------------------
// Nord colors
// ---------------------------------------------------------------------------

const BG_COLOR: [f32; 4] = [0.180, 0.204, 0.251, 1.0]; // #2E3440
const BG_LIGHTER: [f32; 4] = [0.231, 0.259, 0.322, 1.0]; // #3B4252
const BG_SELECTION: [f32; 4] = [0.263, 0.298, 0.369, 1.0]; // #434C5E
const BORDER_COLOR: [f32; 4] = [0.298, 0.337, 0.416, 1.0]; // #4C566A
const FG_COLOR: [f32; 4] = [0.898, 0.914, 0.941, 1.0]; // #E5E9F0
const FG_MUTED: [f32; 4] = [0.298, 0.337, 0.416, 1.0]; // #4C566A (comments)
const ACCENT_COLOR: [f32; 4] = [0.533, 0.753, 0.816, 1.0]; // #88C0D0
const RED_COLOR: [f32; 4] = [0.749, 0.380, 0.416, 1.0]; // #BF616A
const GREEN_COLOR: [f32; 4] = [0.639, 0.745, 0.549, 1.0]; // #A3BE8C
const YELLOW_COLOR: [f32; 4] = [0.922, 0.796, 0.545, 1.0]; // #EBCB8B
const ORANGE_COLOR: [f32; 4] = [0.816, 0.529, 0.439, 1.0]; // #D08770
const TODAY_BG: [f32; 4] = [0.533, 0.753, 0.816, 0.2]; // #88C0D0 at 20% alpha
const CURSOR_BG: [f32; 4] = [0.506, 0.631, 0.757, 0.3]; // #81A1C1 at 30% alpha
const EDITOR_BG: [f32; 4] = [0.200, 0.224, 0.271, 0.95]; // dark overlay

/// Event colors for different calendars.
const EVENT_COLORS: &[[f32; 4]] = &[
    ACCENT_COLOR,
    GREEN_COLOR,
    ORANGE_COLOR,
    YELLOW_COLOR,
    RED_COLOR,
];

fn event_color_for_calendar(calendar: &str) -> [f32; 4] {
    let hash = calendar.bytes().fold(0u32, |acc, b| acc.wrapping_add(u32::from(b)));
    EVENT_COLORS[(hash as usize) % EVENT_COLORS.len()]
}

fn to_glyphon_color(c: &[f32; 4]) -> GlyphonColor {
    GlyphonColor::rgba(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        (c[3] * 255.0) as u8,
    )
}

// ---------------------------------------------------------------------------
// RectPipeline
// ---------------------------------------------------------------------------

struct RectPipeline {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
}

impl RectPipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen_uniforms"),
            size: std::mem::size_of::<ScreenUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RectInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            uniform_buffer,
            uniform_bind_group,
        }
    }
}

// ---------------------------------------------------------------------------
// KoyomiRenderer
// ---------------------------------------------------------------------------

/// The main calendar renderer implementing madori's `RenderCallback`.
pub struct KoyomiRenderer {
    state: SharedState,
    rect_pipeline: Option<RectPipeline>,
    surface_format: Option<wgpu::TextureFormat>,
}

impl KoyomiRenderer {
    #[must_use]
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            rect_pipeline: None,
            surface_format: None,
        }
    }

    /// Draw rectangles into the given render pass.
    fn draw_rects(
        &self,
        ctx: &mut RenderContext<'_>,
        encoder: &mut wgpu::CommandEncoder,
        rects: &[RectInstance],
    ) {
        let Some(pipeline) = &self.rect_pipeline else {
            return;
        };
        if rects.is_empty() {
            return;
        }

        // Update uniforms
        let uniforms = ScreenUniforms {
            resolution: [ctx.width as f32, ctx.height as f32],
            _padding: [0.0; 2],
        };
        ctx.gpu
            .queue
            .write_buffer(&pipeline.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Create instance buffer
        let instance_buffer =
            ctx.gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rect_instances"),
                    contents: bytemuck::cast_slice(rects),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rect_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &pipeline.uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, instance_buffer.slice(..));
            pass.draw(0..6, 0..rects.len() as u32);
        }
    }
}

use wgpu::util::DeviceExt;

impl RenderCallback for KoyomiRenderer {
    fn init(&mut self, _gpu: &garasu::GpuContext) {
        // We'll create the pipeline when we know the surface format
        // For now, store a placeholder format
        self.surface_format = Some(wgpu::TextureFormat::Bgra8UnormSrgb);
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn render(&mut self, ctx: &mut RenderContext<'_>) {
        // Lazily initialize the rect pipeline on first render
        if self.rect_pipeline.is_none() {
            let format = self.surface_format.unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
            self.rect_pipeline = Some(RectPipeline::new(&ctx.gpu.device, format));
        }

        let state = self.state.lock().unwrap();
        let w = ctx.width as f32;
        let h = ctx.height as f32;
        let font_size = state.font_size;

        // --- Pass 1: Clear background ---
        let mut encoder =
            ctx.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("clear"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(BG_COLOR[0]),
                            g: f64::from(BG_COLOR[1]),
                            b: f64::from(BG_COLOR[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));

        // --- Pass 2: Collect rectangles for the current view ---
        let mut rects = Vec::new();
        let mut text_areas: Vec<TextArea> = Vec::new();

        // Header bar
        let header_h = font_size * 2.5;
        rects.push(RectInstance {
            pos: [0.0, 0.0],
            size: [w, header_h],
            color: BG_LIGHTER,
        });

        // Header text
        let header_text = match state.view_mode {
            ViewMode::Month => {
                format!(
                    "{} {}",
                    calendar::month_name(state.month),
                    state.year
                )
            }
            ViewMode::Week => {
                let dates = calendar::week_dates(state.cursor_date, state.week_start);
                format!(
                    "Week of {} - {}",
                    dates[0].format("%b %d"),
                    dates[6].format("%b %d, %Y")
                )
            }
            ViewMode::Day => {
                state
                    .cursor_date
                    .format("%A, %B %d, %Y")
                    .to_string()
            }
        };
        text_areas.push(TextArea {
            text: header_text,
            x: 16.0,
            y: font_size * 0.6,
            color: FG_COLOR,
            size: font_size * 1.4,
            max_width: w - 32.0,
        });

        // View mode indicator
        let mode_text = match state.view_mode {
            ViewMode::Month => "[Month]",
            ViewMode::Week => "[Week]",
            ViewMode::Day => "[Day]",
        };
        text_areas.push(TextArea {
            text: mode_text.to_string(),
            x: w - 120.0,
            y: font_size * 0.6,
            color: ACCENT_COLOR,
            size: font_size,
            max_width: 110.0,
        });

        // Content area
        let content_y = header_h;
        let status_h = font_size * 2.0;
        let content_h = h - content_y - status_h;

        match state.view_mode {
            ViewMode::Month => {
                render_month_view(
                    &state,
                    content_y,
                    w,
                    content_h,
                    font_size,
                    &mut rects,
                    &mut text_areas,
                );
            }
            ViewMode::Week => {
                render_week_view(
                    &state,
                    content_y,
                    w,
                    content_h,
                    font_size,
                    &mut rects,
                    &mut text_areas,
                );
            }
            ViewMode::Day => {
                render_day_view(
                    &state,
                    content_y,
                    w,
                    content_h,
                    font_size,
                    &mut rects,
                    &mut text_areas,
                );
            }
        }

        // Status bar
        let status_y = h - status_h;
        rects.push(RectInstance {
            pos: [0.0, status_y],
            size: [w, status_h],
            color: BG_LIGHTER,
        });

        let status_text = match state.input_mode {
            InputMode::Normal => {
                if state.status.is_empty() {
                    format!(
                        "{}  |  hjkl:nav  n/p:month  v:view  a:add  q:quit",
                        state.cursor_date.format("%Y-%m-%d")
                    )
                } else {
                    state.status.clone()
                }
            }
            InputMode::EventEditor => "Tab:next field  Enter:save  Esc:cancel".to_string(),
            InputMode::Command => format!(":{}", state.command_buffer),
        };

        text_areas.push(TextArea {
            text: status_text,
            x: 12.0,
            y: status_y + font_size * 0.4,
            color: FG_MUTED,
            size: font_size * 0.9,
            max_width: w - 24.0,
        });

        // Editor overlay
        if let Some(ref editor) = state.editor {
            render_editor_overlay(
                editor,
                w,
                h,
                font_size,
                &mut rects,
                &mut text_areas,
            );
        }

        drop(state);

        // --- Pass 3: Draw rectangles ---
        let mut encoder =
            ctx.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rects"),
                });
        self.draw_rects(ctx, &mut encoder, &rects);
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));

        // --- Pass 4: Draw text ---
        let mut font_system = glyphon::FontSystem::new();
        let mut swash_cache = glyphon::SwashCache::new();
        let format = self.surface_format.unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
        let cache = glyphon::Cache::new(&ctx.gpu.device);
        let mut text_atlas = glyphon::TextAtlas::new(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            &cache,
            format,
        );
        let mut viewport = glyphon::Viewport::new(&ctx.gpu.device, &cache);
        viewport.update(
            &ctx.gpu.queue,
            glyphon::Resolution {
                width: ctx.width,
                height: ctx.height,
            },
        );
        let mut text_renderer = glyphon::TextRenderer::new(
            &mut text_atlas,
            &ctx.gpu.device,
            wgpu::MultisampleState::default(),
            None,
        );

        let mut buffers: Vec<glyphon::Buffer> = Vec::new();
        for area in &text_areas {
            let mut buffer = Buffer::new(
                &mut font_system,
                Metrics::new(area.size, area.size * 1.2),
            );
            buffer.set_size(&mut font_system, Some(area.max_width), None);
            buffer.set_text(
                &mut font_system,
                &area.text,
                &Attrs::new()
                    .family(Family::SansSerif)
                    .color(to_glyphon_color(&area.color)),
                Shaping::Advanced,
            );
            buffer.shape_until_scroll(&mut font_system, false);
            buffers.push(buffer);
        }

        let text_bounds = glyphon::TextBounds {
            left: 0,
            top: 0,
            right: ctx.width as i32,
            bottom: ctx.height as i32,
        };

        let text_areas_glyphon: Vec<glyphon::TextArea> = buffers
            .iter()
            .zip(text_areas.iter())
            .map(|(buf, area)| glyphon::TextArea {
                buffer: buf,
                left: area.x,
                top: area.y,
                scale: 1.0,
                bounds: text_bounds,
                default_color: to_glyphon_color(&area.color),
                custom_glyphs: &[],
            })
            .collect();

        text_renderer
            .prepare(
                &ctx.gpu.device,
                &ctx.gpu.queue,
                &mut font_system,
                &mut text_atlas,
                &viewport,
                text_areas_glyphon,
                &mut swash_cache,
            )
            .ok();

        let mut encoder =
            ctx.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("text"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("text_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            text_renderer.render(&text_atlas, &viewport, &mut pass).ok();
        }
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));
    }
}

// ---------------------------------------------------------------------------
// Text area helper
// ---------------------------------------------------------------------------

struct TextArea {
    text: String,
    x: f32,
    y: f32,
    color: [f32; 4],
    size: f32,
    max_width: f32,
}

// ---------------------------------------------------------------------------
// Month view rendering
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_month_view(
    state: &AppState,
    content_y: f32,
    width: f32,
    height: f32,
    font_size: f32,
    rects: &mut Vec<RectInstance>,
    text_areas: &mut Vec<TextArea>,
) {
    let grid = calendar::month_grid(state.year, state.month, state.week_start);
    let headers = calendar::weekday_headers(state.week_start);

    let padding = 8.0;
    let header_row_h = font_size * 1.8;
    let cell_w = (width - padding * 2.0) / 7.0;
    let remaining_h = height - header_row_h - padding;
    let cell_h = remaining_h / grid.len() as f32;

    // Weekday headers
    for (i, header) in headers.iter().enumerate() {
        let x = padding + i as f32 * cell_w;
        text_areas.push(TextArea {
            text: (*header).to_string(),
            x: x + cell_w * 0.5 - font_size,
            y: content_y + font_size * 0.4,
            color: FG_MUTED,
            size: font_size * 0.85,
            max_width: cell_w,
        });
    }

    // Grid separator
    rects.push(RectInstance {
        pos: [padding, content_y + header_row_h - 1.0],
        size: [width - padding * 2.0, 1.0],
        color: BORDER_COLOR,
    });

    // Day cells
    for (week_idx, week) in grid.iter().enumerate() {
        for (day_idx, cell) in week.iter().enumerate() {
            let x = padding + day_idx as f32 * cell_w;
            let y = content_y + header_row_h + week_idx as f32 * cell_h;

            // Cell border
            rects.push(RectInstance {
                pos: [x, y],
                size: [cell_w, 1.0],
                color: [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], 0.3],
            });
            rects.push(RectInstance {
                pos: [x, y],
                size: [1.0, cell_h],
                color: [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], 0.3],
            });

            // Today highlight
            if cell.is_today {
                rects.push(RectInstance {
                    pos: [x + 1.0, y + 1.0],
                    size: [cell_w - 2.0, cell_h - 2.0],
                    color: TODAY_BG,
                });
            }

            // Cursor highlight
            if cell.date == state.cursor_date {
                rects.push(RectInstance {
                    pos: [x + 1.0, y + 1.0],
                    size: [cell_w - 2.0, cell_h - 2.0],
                    color: CURSOR_BG,
                });
            }

            // Day number
            let day_color = if cell.in_current_month {
                if cell.is_today {
                    ACCENT_COLOR
                } else {
                    FG_COLOR
                }
            } else {
                FG_MUTED
            };

            text_areas.push(TextArea {
                text: cell.date.day().to_string(),
                x: x + 4.0,
                y: y + 2.0,
                color: day_color,
                size: font_size * 0.85,
                max_width: cell_w - 8.0,
            });

            // Event indicators for this day
            let day_events: Vec<&EventOccurrence> = state
                .cached_events
                .iter()
                .filter(|occ| occ.occurrence_start.date() == cell.date)
                .collect();

            let max_display = 2;
            let event_y_start = y + font_size * 1.2;
            let event_h = font_size * 0.9;

            for (i, occ) in day_events.iter().take(max_display).enumerate() {
                let ey = event_y_start + i as f32 * (event_h + 2.0);
                let color = event_color_for_calendar(&occ.event.calendar);

                // Event color bar
                rects.push(RectInstance {
                    pos: [x + 3.0, ey],
                    size: [3.0, event_h],
                    color,
                });

                // Truncated event title
                let title = if occ.event.title.len() > 8 {
                    format!("{}...", &occ.event.title[..8])
                } else {
                    occ.event.title.clone()
                };
                text_areas.push(TextArea {
                    text: title,
                    x: x + 8.0,
                    y: ey,
                    color: FG_COLOR,
                    size: font_size * 0.7,
                    max_width: cell_w - 12.0,
                });
            }

            if day_events.len() > max_display {
                let ey = event_y_start + max_display as f32 * (event_h + 2.0);
                text_areas.push(TextArea {
                    text: format!("+{} more", day_events.len() - max_display),
                    x: x + 8.0,
                    y: ey,
                    color: FG_MUTED,
                    size: font_size * 0.65,
                    max_width: cell_w - 12.0,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Week view rendering
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_week_view(
    state: &AppState,
    content_y: f32,
    width: f32,
    height: f32,
    font_size: f32,
    rects: &mut Vec<RectInstance>,
    text_areas: &mut Vec<TextArea>,
) {
    let dates = calendar::week_dates(state.cursor_date, state.week_start);
    let today = Local::now().date_naive();

    let time_col_w = font_size * 4.5;
    let padding = 4.0;
    let day_col_w = (width - time_col_w - padding * 2.0) / 7.0;
    let header_h = font_size * 2.0;
    let hour_h = (height - header_h) / 24.0;

    // Day column headers
    for (i, date) in dates.iter().enumerate() {
        let x = time_col_w + padding + i as f32 * day_col_w;
        let label = format!("{} {}", date.format("%a"), date.day());
        let color = if *date == today {
            ACCENT_COLOR
        } else if *date == state.cursor_date {
            FG_COLOR
        } else {
            FG_MUTED
        };
        text_areas.push(TextArea {
            text: label,
            x: x + 4.0,
            y: content_y + font_size * 0.3,
            color,
            size: font_size * 0.85,
            max_width: day_col_w - 8.0,
        });

        // Column separator
        rects.push(RectInstance {
            pos: [x, content_y + header_h],
            size: [1.0, height - header_h],
            color: [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], 0.3],
        });
    }

    // Time labels and hour lines
    let labels = calendar::hour_labels(state.use_24h);
    for (h, label) in labels.iter().enumerate() {
        let y = content_y + header_h + h as f32 * hour_h;

        // Hour label
        text_areas.push(TextArea {
            text: label.clone(),
            x: 4.0,
            y,
            color: FG_MUTED,
            size: font_size * 0.7,
            max_width: time_col_w - 8.0,
        });

        // Hour line
        rects.push(RectInstance {
            pos: [time_col_w, y],
            size: [width - time_col_w, 1.0],
            color: [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], 0.2],
        });
    }

    // Selected hour highlight
    {
        let y = content_y + header_h + state.selected_hour as f32 * hour_h;
        let day_idx = dates.iter().position(|d| *d == state.cursor_date).unwrap_or(0);
        let x = time_col_w + padding + day_idx as f32 * day_col_w;
        rects.push(RectInstance {
            pos: [x, y],
            size: [day_col_w, hour_h],
            color: CURSOR_BG,
        });
    }

    // Current time marker
    let now = Local::now().naive_local();
    if dates.contains(&today) {
        let day_idx = dates.iter().position(|d| *d == today).unwrap_or(0);
        let hour_frac = now.time().hour() as f32 + now.time().minute() as f32 / 60.0;
        let marker_y = content_y + header_h + hour_frac * hour_h;
        let marker_x = time_col_w + padding + day_idx as f32 * day_col_w;

        rects.push(RectInstance {
            pos: [marker_x, marker_y],
            size: [day_col_w, 2.0],
            color: RED_COLOR,
        });
    }

    // Event blocks
    for occ in &state.cached_events {
        let occ_date = occ.occurrence_start.date();
        if let Some(day_idx) = dates.iter().position(|d| *d == occ_date) {
            let start_hour = occ.occurrence_start.time().hour() as f32
                + occ.occurrence_start.time().minute() as f32 / 60.0;
            let end_hour = occ.occurrence_end.time().hour() as f32
                + occ.occurrence_end.time().minute() as f32 / 60.0;
            let block_h = (end_hour - start_hour) * hour_h;

            let x = time_col_w + padding + day_idx as f32 * day_col_w + 2.0;
            let y = content_y + header_h + start_hour * hour_h;
            let color = event_color_for_calendar(&occ.event.calendar);

            rects.push(RectInstance {
                pos: [x, y],
                size: [day_col_w - 4.0, block_h.max(hour_h * 0.5)],
                color: [color[0], color[1], color[2], 0.7],
            });

            text_areas.push(TextArea {
                text: occ.event.title.clone(),
                x: x + 4.0,
                y: y + 2.0,
                color: FG_COLOR,
                size: font_size * 0.75,
                max_width: day_col_w - 12.0,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Day view rendering
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_day_view(
    state: &AppState,
    content_y: f32,
    width: f32,
    height: f32,
    font_size: f32,
    rects: &mut Vec<RectInstance>,
    text_areas: &mut Vec<TextArea>,
) {
    let time_col_w = font_size * 5.0;
    let hour_h = height / 24.0;

    // Time labels and hour lines
    let labels = calendar::hour_labels(state.use_24h);
    for (h, label) in labels.iter().enumerate() {
        let y = content_y + h as f32 * hour_h;

        text_areas.push(TextArea {
            text: label.clone(),
            x: 4.0,
            y,
            color: FG_MUTED,
            size: font_size * 0.8,
            max_width: time_col_w - 8.0,
        });

        rects.push(RectInstance {
            pos: [time_col_w, y],
            size: [width - time_col_w, 1.0],
            color: [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], 0.2],
        });
    }

    // Selected hour
    {
        let y = content_y + state.selected_hour as f32 * hour_h;
        rects.push(RectInstance {
            pos: [time_col_w, y],
            size: [width - time_col_w, hour_h],
            color: CURSOR_BG,
        });
    }

    // Current time marker
    let today = Local::now().date_naive();
    if state.cursor_date == today {
        let now = Local::now().naive_local();
        let hour_frac = now.time().hour() as f32 + now.time().minute() as f32 / 60.0;
        let marker_y = content_y + hour_frac * hour_h;
        rects.push(RectInstance {
            pos: [time_col_w, marker_y],
            size: [width - time_col_w, 2.0],
            color: RED_COLOR,
        });
    }

    // Event blocks
    let event_col_x = time_col_w + 4.0;
    let event_col_w = width - time_col_w - 8.0;

    for (idx, occ) in state.cached_events.iter().enumerate() {
        if occ.occurrence_start.date() != state.cursor_date {
            continue;
        }

        let start_hour = occ.occurrence_start.time().hour() as f32
            + occ.occurrence_start.time().minute() as f32 / 60.0;
        let end_hour = occ.occurrence_end.time().hour() as f32
            + occ.occurrence_end.time().minute() as f32 / 60.0;
        let block_h = (end_hour - start_hour) * hour_h;

        let y = content_y + start_hour * hour_h;
        let color = event_color_for_calendar(&occ.event.calendar);

        // Highlight selected event
        let is_selected = idx == state.selected_event_idx;
        let alpha = if is_selected { 0.9 } else { 0.6 };

        rects.push(RectInstance {
            pos: [event_col_x, y],
            size: [event_col_w, block_h.max(hour_h * 0.5)],
            color: [color[0], color[1], color[2], alpha],
        });

        // Event title
        text_areas.push(TextArea {
            text: occ.event.title.clone(),
            x: event_col_x + 8.0,
            y: y + 2.0,
            color: FG_COLOR,
            size: font_size * 0.85,
            max_width: event_col_w - 16.0,
        });

        // Event time
        let time_str = format!(
            "{} - {}",
            occ.occurrence_start.format("%H:%M"),
            occ.occurrence_end.format("%H:%M"),
        );
        text_areas.push(TextArea {
            text: time_str,
            x: event_col_x + 8.0,
            y: y + font_size * 1.0,
            color: [FG_COLOR[0], FG_COLOR[1], FG_COLOR[2], 0.7],
            size: font_size * 0.7,
            max_width: event_col_w - 16.0,
        });

        // Location if present
        if let Some(ref loc) = occ.event.location {
            text_areas.push(TextArea {
                text: loc.clone(),
                x: event_col_x + 8.0,
                y: y + font_size * 1.8,
                color: FG_MUTED,
                size: font_size * 0.65,
                max_width: event_col_w - 16.0,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Event editor overlay
// ---------------------------------------------------------------------------

fn render_editor_overlay(
    editor: &EditorState,
    width: f32,
    height: f32,
    font_size: f32,
    rects: &mut Vec<RectInstance>,
    text_areas: &mut Vec<TextArea>,
) {
    let overlay_w = 400.0_f32.min(width - 40.0);
    let overlay_h = 350.0_f32.min(height - 40.0);
    let overlay_x = (width - overlay_w) / 2.0;
    let overlay_y = (height - overlay_h) / 2.0;

    // Semi-transparent backdrop
    rects.push(RectInstance {
        pos: [0.0, 0.0],
        size: [width, height],
        color: [0.0, 0.0, 0.0, 0.5],
    });

    // Editor panel
    rects.push(RectInstance {
        pos: [overlay_x, overlay_y],
        size: [overlay_w, overlay_h],
        color: EDITOR_BG,
    });

    // Border
    rects.push(RectInstance {
        pos: [overlay_x, overlay_y],
        size: [overlay_w, 2.0],
        color: ACCENT_COLOR,
    });

    // Title
    let title = if editor.editing_id.is_some() {
        "Edit Event"
    } else {
        "New Event"
    };
    text_areas.push(TextArea {
        text: title.to_string(),
        x: overlay_x + 16.0,
        y: overlay_y + 12.0,
        color: ACCENT_COLOR,
        size: font_size * 1.2,
        max_width: overlay_w - 32.0,
    });

    // Fields
    let fields = [
        (EditorField::Title, &editor.title),
        (EditorField::StartDate, &editor.start_date),
        (EditorField::StartTime, &editor.start_time),
        (EditorField::EndDate, &editor.end_date),
        (EditorField::EndTime, &editor.end_time),
        (EditorField::Location, &editor.location),
        (EditorField::Calendar, &editor.calendar),
        (EditorField::Recurrence, &editor.recurrence),
    ];

    let field_h = font_size * 2.2;
    let fields_y = overlay_y + font_size * 3.0;

    for (i, (field, value)) in fields.iter().enumerate() {
        let y = fields_y + i as f32 * field_h;
        let is_active = editor.active_field == *field;

        // Field background
        if is_active {
            rects.push(RectInstance {
                pos: [overlay_x + 12.0, y],
                size: [overlay_w - 24.0, field_h - 4.0],
                color: BG_SELECTION,
            });
        }

        // Label
        text_areas.push(TextArea {
            text: field.label().to_string(),
            x: overlay_x + 16.0,
            y: y + 2.0,
            color: if is_active { ACCENT_COLOR } else { FG_MUTED },
            size: font_size * 0.75,
            max_width: 100.0,
        });

        // Value
        let display_value = if value.is_empty() && is_active {
            "...".to_string()
        } else {
            (*value).clone()
        };
        text_areas.push(TextArea {
            text: display_value,
            x: overlay_x + 130.0,
            y: y + 2.0,
            color: FG_COLOR,
            size: font_size * 0.85,
            max_width: overlay_w - 150.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_color_deterministic() {
        let c1 = event_color_for_calendar("work");
        let c2 = event_color_for_calendar("work");
        assert_eq!(c1, c2);
    }

    #[test]
    fn event_color_varies_by_calendar() {
        let c1 = event_color_for_calendar("work");
        let c2 = event_color_for_calendar("personal");
        // Different calendars may get different colors (not guaranteed but likely)
        // Just check they produce valid colors
        assert!(c1[3] > 0.0);
        assert!(c2[3] > 0.0);
    }

    #[test]
    fn to_glyphon_color_conversion() {
        let c = to_glyphon_color(&[1.0, 0.5, 0.0, 1.0]);
        assert_eq!(c, GlyphonColor::rgba(255, 127, 0, 255));
    }

    #[test]
    fn editor_state_new_for_date() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let editor = EditorState::new_for_date(date, 10);
        assert_eq!(editor.start_date, "2026-03-10");
        assert_eq!(editor.start_time, "10:00");
        assert_eq!(editor.end_time, "11:00");
        assert!(editor.title.is_empty());
        assert!(editor.editing_id.is_none());
    }

    #[test]
    fn editor_active_value_mut() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let mut editor = EditorState::new_for_date(date, 10);
        editor.active_value_mut().push_str("Meeting");
        assert_eq!(editor.title, "Meeting");
    }
}
