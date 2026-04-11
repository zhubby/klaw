# WASM 中文字体零打包方案

## 问题背景

当前 klaw-webui 在 WASM 构建中内嵌了 LXGW WenKai 字体（约 2-3 MB gzipped），用于渲染中文文本。这带来以下问题：

1. **WASM 包体积膨胀**：字体数据显著增加初始加载时间
2. **字体栅格化开销**：egui 使用 fontdue 在 CPU 上栅格化，滚动时可能有卡顿
3. **缺少浏览器级 fallback**：无法利用系统的苹方/微软雅黑/Noto CJK 等优化字体

## 目标

在浏览器中运行 eframe/egui WASM 应用时：
- 不打包任何 CJK 字体
- 使用浏览器系统字体渲染中文
- 保持布局正确性

## 核心思想

让 **egui 只负责布局（layout）**，让 **浏览器 Canvas2D 负责真正的文本绘制（fillText）**。

```
egui → 生成 Shape::Text + galley
        ↓
      拦截 primitives
        ↓
      CanvasRenderingContext2D.fillText()
```

egui 不再栅格字体、不再构建 atlas、不再上传 WebGL 纹理。

## 架构限制分析

### eframe 渲染管道

```
┌─────────────────────────────────────────────────────────┐
│                    WebRunner::start()                    │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  AppRunner::logic()                                      │
│    - app.update(ctx)                                     │
│    - ctx.run(input) → FullOutput { shapes, ... }        │
│    - ctx.tessellate(shapes) → clipped_primitives        │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  AppRunner::paint()                                      │
│    - painter.paint_and_update_textures(                  │
│        clipped_primitives,  ← 已栅格化，无拦截点          │
│        textures_delta,                                   │
│      )                                                   │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│  WebPainterWgpu::paint_and_update_textures()            │
│    - 创建 wgpu render pass                               │
│    - renderer.render(clipped_primitives)                 │
│    - 直接 GPU 渲染，文本已转为三角形                      │
└─────────────────────────────────────────────────────────┘
```

**关键问题**：`WebPainter` trait 是 `pub(crate)`，无法从外部覆盖。

```rust
// eframe-0.33.3/src/web/web_painter.rs:7
pub(crate) trait WebPainter {
    fn paint_and_update_textures(
        &mut self,
        clipped_primitives: &[egui::ClippedPrimitive], // ← 文本已栅格化
        ...
    ) -> Result<(), JsValue>;
}
```

## 实现方案

### 方案 A：自定义渲染层（推荐）

修改 eframe 渲染管道，在 `tessellate` 后、`paint` 前拦截文本 primitive：

#### 1. klaw-ui-kit：字体配置

```rust
// klaw-ui-kit/src/fonts.rs

#[cfg(target_arch = "wasm32")]
pub fn install_fonts(ctx: &egui::Context) {
    // 使用最小字体集，仅用于布局测量
    let mut fonts = egui::FontDefinitions::empty();
    
    // 添加 Phosphor 图标字体（必须，UI 依赖）
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    
    // 可选：添加极小的 ASCII-only 字体用于布局基准
    // fonts.font_data.insert(
    //     "layout-baseline".to_string(),
    //     egui::FontData::from_static(MINIMAL_ASCII_FONT).into(),
    // );
    
    ctx.set_fonts(fonts);
}

/// WASM 专用：返回用于 Canvas2D 的 CSS 字体声明
#[cfg(target_arch = "wasm32")]
pub fn browser_font_family(family: egui::FontFamily) -> &'static str {
    match family {
        egui::FontFamily::Proportional => {
            "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', \
             'PingFang SC', 'Microsoft YaHei', 'Noto Sans CJK SC', sans-serif"
        }
        egui::FontFamily::Monospace => {
            "'LXGW WenKai Mono', 'SF Mono', Monaco, 'Cascadia Code', \
             'PingFang SC', 'Microsoft YaHei', monospace"
        }
        _ => "system-ui, sans-serif",
    }
}
```

#### 2. klaw-webui：Canvas2D 文本渲染器

```rust
// klaw-webui/src/web_chat/canvas_text.rs

use egui::{ClippedPrimitive, Shape, Pos2, Vec2};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use wasm_bindgen::JsCast;
use klaw_ui_kit::browser_font_family;

pub struct CanvasTextRenderer {
    ctx: CanvasRenderingContext2d,
    canvas: HtmlCanvasElement,
}

impl CanvasTextRenderer {
    pub fn new(canvas: HtmlCanvasElement) -> Self {
        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();
        Self { ctx, canvas }
    }

    /// 从 clipped_primitives 中分离文本和其他图元
    pub fn separate_text_shapes(
        primitives: &[ClippedPrimitive],
    ) -> (Vec<TextDrawCommand>, Vec<ClippedPrimitive>) {
        let mut text_commands = Vec::new();
        let mut other_primitives = Vec::new();

        for clipped in primitives {
            match &clipped.primitive {
                Shape::Text(text_shape) => {
                    for row in &text_shape.galley.rows {
                        text_commands.push(TextDrawCommand {
                            pos: text_shape.pos + row.pos.to_vec2(),
                            text: row.text().to_string(),
                            font_size: text_shape.galley.glyph_widths
                                .first()
                                .map(|w| w as f64)
                                .unwrap_or(14.0),
                            family: text_shape.galley
                                .elders
                                .first()
                                .and_then(|el| el.style.font_id.as_ref())
                                .map(|id| id.family.clone())
                                .unwrap_or(egui::FontFamily::Proportional),
                            color: text_shape.override_text_color
                                .unwrap_or_else(|| text_shape.shape.color),
                        });
                    }
                }
                _ => {
                    other_primitives.push(clipped.clone());
                }
            }
        }

        (text_commands, other_primitives)
    }

    /// 使用 Canvas2D 绘制文本
    pub fn draw_text(&self, cmd: &TextDrawCommand, pixels_per_point: f32) {
        let font_family = browser_font_family(cmd.family);
        let font_size = cmd.font_size * pixels_per_point as f64;
        
        self.ctx.set_font(&format!(
            "{font_size:.1}px {font_family}"
        ));
        self.ctx.set_text_baseline("top");
        self.ctx.set_fill_style(&cmd.color.to_string());

        let _ = self.ctx.fill_text(
            &cmd.text,
            cmd.pos.x as f64 * pixels_per_point as f64,
            cmd.pos.y as f64 * pixels_per_point as f64,
        );
    }

    /// 批量绘制所有文本
    pub fn draw_all_text(&self, commands: &[TextDrawCommand], pixels_per_point: f32) {
        for cmd in commands {
            self.draw_text(cmd, pixels_per_point);
        }
    }
}

#[derive(Debug)]
pub struct TextDrawCommand {
    pub pos: Pos2,
    pub text: String,
    pub font_size: f64,
    pub family: egui::FontFamily,
    pub color: egui::Color32,
}
```

#### 3. 自定义 WebRunner（关键）

需要 fork 或包装 eframe 的 WebRunner：

```rust
// klaw-webui/src/web_chat/custom_runner.rs

use eframe::WebRunner;
use crate::canvas_text::CanvasTextRenderer;

/// 包装 WebRunner，在渲染前拦截文本
pub struct KlawWebRunner {
    inner: WebRunner,
    text_renderer: Option<CanvasTextRenderer>,
}

impl KlawWebRunner {
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
        web_options: eframe::WebOptions,
        app_creator: eframe::AppCreator<'static>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        // 初始化文本渲染器
        let text_renderer = CanvasTextRenderer::new(canvas.clone());
        
        // ... 需要访问 inner AppRunner 的 logic() 输出
        // 这需要修改 eframe 源码或使用 glow backend
    }
}
```

**问题**：当前的 eframe API 不暴露 `clipped_primitives`。

#### 4. 替代方案：使用 glow backend

glow backend 可能更容易拦截：

```toml
# klaw-webui/Cargo.toml
[dependencies]
eframe = { version = "0.33", features = ["glow"] }  # 使用 glow 而非 wgpu
```

glow backend 的渲染流程可能提供更多控制点。

### 方案 B：字体子集化（折中方案）

如果无法修改渲染管道，使用字体子集化减少包体积：

```rust
// 构建时只包含常用 CJK 字符
// 使用 fonttools 或 pyftsubset 预处理
// 将 LXGW WenKai 从 ~8MB 减少到 ~500KB（覆盖 3500 常用字）
```

### 方案 C：CSS Font Loading + 缓存（运行时加载）

```rust
// klaw-ui-kit/src/fonts.rs

#[cfg(target_arch = "wasm32")]
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // 仅保留必需的图标字体
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    
    // 使用浏览器 CSS font loading API 动态加载
    // 首次渲染时可能显示 fallback 字体，之后缓存
    wasm_bindgen_futures::spawn_local(async {
        load_system_cjk_fonts_via_css().await;
    });
    
    ctx.set_fonts(fonts);
}

#[cfg(target_arch = "wasm32")]
async fn load_system_cjk_fonts_via_css() {
    use web_sys::window;
    
    // 使用 CSS Font Loading API
    let document = window().unwrap().document().unwrap();
    let style = document.create_element("style").unwrap();
    style.set_inner_html(r#"
        @font-face {
            font-family: 'SystemCJK';
            src: local('PingFang SC'), 
                 local('Microsoft YaHei'),
                 local('Noto Sans CJK SC'),
                 local('Hiragino Sans GB');
        }
    "#);
    document.head().unwrap().append_child(&style).unwrap();
}
```

## 效果对比

| 项目 | 当前方案（内嵌字体） | Canvas2D 方案 | 字体子集化 |
|------|---------------------|--------------|-----------|
| 中文支持 | 完美 | 完美 | 覆盖常用字 |
| WASM 体积 | +2~3 MB | 不变 | +500 KB |
| CPU 占用 | 高（fontdue栅格） | 低（浏览器原生） | 高 |
| 滚动流畅度 | 可能卡顿 | 流畅 | 可能卡顿 |
| 字体 fallback | 无 | 浏览器级 | 无 |
| 实现复杂度 | 低 | 高（需改 eframe） | 中 |

## 实施路径

### 第一阶段：当前状态

- ✅ 使用内嵌 LXGW WenKai 字体
- ✅ 桌面端系统 CJK fallback
- ✅ WASM 分支已分离

### 第二阶段：字体优化（立即可行）

1. 字体子集化：减少内嵌字体体积
2. 延迟加载：只在需要中文时加载
3. 使用 WOFF2 格式压缩

### 第三阶段：Canvas2D 方案（需上游支持）

1. 跟踪 eframe issue/PR 讨论 WebPainter 扩展
2. 或切换到 glow backend 尝试更多控制
3. 实现文本 Shape 拦截和 Canvas2D 渲染

## 相关资源

- [egui WebPainter trait](https://github.com/emilk/egui/blob/master/crates/eframe/src/web/web_painter.rs)
- [egui Shape::Text](https://github.com/emilk/egui/blob/master/crates/epaint/src/text/galley.rs)
- [Font Loading API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Font_Loading_API)

## 当前代码位置

| 模块 | 路径 | 作用 |
|------|------|------|
| 字体安装入口 | `klaw-ui-kit/src/fonts.rs` | 桌面/WASM 分支 |
| WASM 启动 | `klaw-webui/src/web_chat/mod.rs` | WebRunner 初始化 |
| eframe WebRunner | `eframe::WebRunner` | 渲染管道控制 |