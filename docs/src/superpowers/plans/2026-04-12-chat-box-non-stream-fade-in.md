# Chat Box Non-Stream Fade-In Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reuse `klaw-ui-kit`'s `FadeIn` text effect so newly arrived non-stream assistant messages animate once in the GUI chat box.

**Architecture:** Keep animation state local to `ChatBox` instead of persisting it on `ChatMessage`. Detect newly appended assistant non-stream messages, allocate a `TextAnimator` keyed by message id, render animated text while active, then fall back to normal text after completion. Historical messages loaded into a chat box should not animate.

**Tech Stack:** Rust 2024, `egui`, `klaw-ui-kit::text_animator`

---

### Task 1: Lock down animation trigger behavior

**Files:**
- Modify: `klaw-gui/src/widgets/chat_box.rs`
- Test: `klaw-gui/src/widgets/chat_box.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tracks_only_new_non_stream_assistant_messages_for_animation() {
    let existing = ChatMessage::assistant("history");
    let mut chat_box = ChatBox::new("Chat").with_messages(vec![existing.clone()]);

    assert!(chat_box.fade_in_messages.is_empty());

    let user_message = ChatMessage::user("hello");
    chat_box.add_message(user_message.clone());
    assert!(chat_box.fade_in_messages.is_empty());

    let streaming_assistant = ChatMessage::assistant("partial").set_streaming(true);
    chat_box.add_message(streaming_assistant.clone());
    assert!(chat_box.fade_in_messages.is_empty());

    let final_assistant = ChatMessage::assistant("final");
    let final_id = final_assistant.id.clone();
    chat_box.add_message(final_assistant);

    assert!(chat_box.fade_in_messages.contains_key(&final_id));
    assert_eq!(chat_box.fade_in_messages.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p klaw-gui tracks_only_new_non_stream_assistant_messages_for_animation`
Expected: FAIL because `ChatBox` does not yet track fade-in animation state.

- [ ] **Step 3: Write minimal implementation**

```rust
fn should_animate_message(message: &ChatMessage) -> bool {
    matches!(message.role, ChatRole::Assistant) && !message.is_streaming && !message.content.is_empty()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p klaw-gui tracks_only_new_non_stream_assistant_messages_for_animation`
Expected: PASS

### Task 2: Render animated assistant text

**Files:**
- Modify: `klaw-gui/src/widgets/chat_box.rs`
- Modify: `klaw-gui/src/widgets/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn retires_fade_in_state_after_animation_finishes() {
    let mut chat_box = ChatBox::new("Chat");
    let message = ChatMessage::assistant("done");
    let id = message.id.clone();
    chat_box.add_message(message);

    let animator = chat_box.fade_in_messages.get_mut(&id).unwrap();
    animator.timer = 1.0;
    animator.animation_finished = true;

    chat_box.prune_finished_animations();

    assert!(!chat_box.fade_in_messages.contains_key(&id));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p klaw-gui retires_fade_in_state_after_animation_finishes`
Expected: FAIL because finished animation state is not removed yet.

- [ ] **Step 3: Write minimal implementation**

```rust
fn prune_finished_animations(&mut self) {
    self.fade_in_messages
        .retain(|_, animator| !animator.is_animation_finished());
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p klaw-gui retires_fade_in_state_after_animation_finishes`
Expected: PASS
