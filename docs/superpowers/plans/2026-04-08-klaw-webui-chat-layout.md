# klaw-webui Chat Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `klaw-webui` 改造成更接近现代聊天产品的三段式界面：紧凑顶栏、居中会话列、独立底部 composer。

**Architecture:** 保持 `ChatApp` 作为状态中心，不改 WebSocket 协议与消息模型；把可测试的文案/状态/布局决策提炼为纯辅助函数，再由 `web_chat.rs` 负责 `egui` 布局渲染。这样既能遵守 TDD，又不把小 crate 过度组件化。

**Tech Stack:** Rust 2024, `eframe`/`egui` web, `wasm-bindgen`, `web_sys::WebSocket`

---

### Task 1: 抽离可测试的展示辅助逻辑

**Files:**
- Create: `klaw-webui/src/presentation.rs`
- Modify: `klaw-webui/src/lib.rs`
- Test: `klaw-webui/src/presentation.rs`

- [ ] **Step 1: 写失败测试**
- [ ] **Step 2: 运行测试确认因新展示行为未实现而失败**
- [ ] **Step 3: 实现最小辅助类型与函数**
- [ ] **Step 4: 再跑测试确认通过**

### Task 2: 实现新的三段式聊天布局

**Files:**
- Modify: `klaw-webui/src/web_chat.rs`
- Use: `klaw-webui/src/presentation.rs`

- [ ] **Step 1: 将 `ConnState` 接入展示辅助逻辑**
- [ ] **Step 2: 把页面拆成顶栏 / 中间消息列 / 底部 composer**
- [ ] **Step 3: 为空状态、断线态、错误态接入新的文案与布局**
- [ ] **Step 4: 复查输入发送与滚动行为未退化**

### Task 3: 验证与收尾

**Files:**
- Modify if needed: `klaw-webui/src/web_chat.rs`, `klaw-webui/src/presentation.rs`

- [ ] **Step 1: 运行 `cargo fmt --all`**
- [ ] **Step 2: 运行 `cargo test -p klaw-webui`**
- [ ] **Step 3: 运行 `make webui-wasm`**
- [ ] **Step 4: 如有回归，再运行相关检查并修正**
