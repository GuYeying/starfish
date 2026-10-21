# Pygame mixer API 覆盖分析

> 对照 `pygame.mixer` 和 `pygame.mixer.music` 分析原型差距。
> ✅ = 已覆盖，🟡 = 部分覆盖，❌ = 未覆盖，🔵 = 底层不需要

---

## 一、pygame.mixer（顶层 API）

| pygame API | 状态 | 对应原型 | 说明 |
|---|---|---|---|
| `mixer.init()` | ✅ | `AudioEngine::new()` | 创建引擎时自动初始化 |
| `mixer.pre_init()` | 🔵 | — | 底层 API 不需要，Cargo.toml 配好就行 |
| `mixer.quit()` | ✅ | `AudioEngine` drop | 析构自动关闭设备 |
| `mixer.get_init()` | ✅ | `AudioEngine` 持有 | 创建成功即已初始化 |
| `mixer.stop()` | ✅ | `engine.stop_all()` |  |
| `mixer.pause()` | ✅ | `engine.pause_all()` |  |
| `mixer.unpause()` | ✅ | `engine.resume_all()` |  |
| `mixer.fadeout()` | ❌ | — | 缺少全局淡出（逐步降低 master_volume） |
| `mixer.set_num_channels()` | ✅ | `AudioEngine::new(n)` | 创建时指定；缺少运行时动态调整 |
| `mixer.get_num_channels()` | ✅ | `engine.num_channels()` | ✅ |
| `mixer.set_reserved()` | ✅ | `engine.set_reserved(n)` | ✅ |
| `mixer.find_channel()` | ✅ | `engine.find_free_channel()` | ✅ |
| `mixer.get_busy()` | ✅ | `engine.get_busy()` | ✅ |
| `mixer.get_sdl_mixer_version()` | 🔵 | — | Rust 侧用 `crate version!()` 即可 |

### 缺失补全

```rust
/// 全局淡出：逐步降低 master_volume 到 0 后停播
/// 需要一个后台任务或定时器，这里用 fade_to 思路：
pub fn fadeout(&mut self, ms: u32) {
    // 设置一个全局淡出目标 → Inner 在 mix 时自动衰减 master
    // 实现方式：Inner.fade_master: Option<FadeState>
}
```

---

## 二、pygame.mixer.Sound

| pygame API | 状态 | 对应原型 | 说明 |
|---|---|---|---|
| `Sound(file)` | 🟡 | `decoder::WavDecoder::from_file()` | 需要补解码器 |
| `Sound(buffer)` | ✅ | `SoundData::from_interleaved_f32()` | 从原始 PCM 创建 |
| `.play(loops, maxtime, fade_ms)` | 🟡 | `engine.play_with(sound, loops, fade_ms, force)` | **缺 `maxtime`**（播 N 毫秒后自动停） |
| `.stop()` | ✅ | `engine.stop(ch)` | 通过 channel 停止 |
| `.fadeout(ms)` | ✅ | `engine.channel_fade_out(ch, ms)` |  |
| `.set_volume()` | ✅ | `engine.set_channel_volume(ch, v)` |  |
| `.get_volume()` | ❌ | — | 缺读取（可加在 SfxChannel 上） |
| `.get_num_channels()` | ❌ | — | 缺查询"这个 Sound 占用几个声道" |
| `.get_length()` | ✅ | `sound.duration()` | ✅ |
| `.get_raw()` | ❌ | — | 缺原始 PCM 导出 |

### 缺失补全

```rust
// 在 SoundData 上加这个：
pub fn raw_f32(&self) -> Vec<f32> {
    let mut out = Vec::with_capacity(self.frames.len() * 2);
    for f in &self.frames {
        out.push(f.left);
        out.push(f.right);
    }
    out
}

// play_with 加 maxtime 参数：
pub fn play_with(
    sound: Arc<SoundData>,
    loops: i32,
    fade_in_ms: f32,
    maxtime_ms: Option<u32>,       // ← 加这个
    force: bool,
) -> Result<usize, AudioError>;

// 实现方式：maxtime 转为帧数上限，在 SfxChannel 上加 max_frames 字段
```

---

## 三、pygame.mixer.Channel

| pygame API | 状态 | 对应原型 | 说明 |
|---|---|---|---|
| `Channel.play(sound, loops, maxtime, fade_ms)` | 🟡 | `engine.play_on(sound, ch)` | 缺 `maxtime` 和 `fade_ms` 参数 |
| `.stop()` | ✅ | `engine.stop(ch)` |  |
| `.pause()` | ✅ | `engine.pause(ch)` |  |
| `.unpause()` | ✅ | `engine.resume(ch)` |  |
| `.fadeout(ms)` | ✅ | `engine.channel_fade_out(ch, ms)` |  |
| `.set_volume(left, right)` | 🟡 | `set_channel_volume()` | **缺独立 L/R 声道音量控制** |
| `.get_volume()` | ❌ | — | 缺读取 |
| `.get_busy()` | ✅ | `engine.is_channel_busy(ch)` |  |
| `.get_sound()` | ❌ | — | 缺"正在播放哪个 Sound" |
| `.queue(sound)` | ❌ | — | 缺声道级别排队 |
| `.get_queue()` | ❌ | — | 缺查看排队列表 |
| `.set_endevent()` / `.get_endevent()` | ❌ | — | 缺播放结束事件 |

### 缺失补全

```rust
// 1. SfxChannel 增加独立 L/R 音量
pub struct SfxChannel {
    pub volume_left: f32,   // 0.0 ~ 1.0（默认为 1.0）
    pub volume_right: f32,  // 0.0 ~ 1.0（默认为 1.0）
}

// 2. 声道排队（playback.rs 已在排队逻辑上接近于做）
impl SfxChannel {
    pub fn queue(&mut self, sound: Arc<SoundData>) {
        self.queue.push_back(sound);
    }
    // read_frames 在播完当前后自动从 queue 取下一首
}

// 3. 播放结束事件（通过 event 系统回调）
// 可以在 AudioEngine 上挂一个事件回调：
pub type EndEventCallback = Box<dyn Fn(usize) + Send>;

pub struct AudioEngine {
    end_event: Option<EndEventCallback>,
    // 当某个 channel 由 Playing → Stopped 时触发
}
```

---

## 四、pygame.mixer.music

| pygame API | 状态 | 对应原型 | 说明 |
|---|---|---|---|
| `music.load(file)` | ✅ | `engine.music_load(data)` | 数据先解码再传入 |
| `music.unload()` | ✅ | `music_load(None)` 或 drop |  |
| `music.play(loops)` | ✅ | `engine.music_play(loops)` | ✅ ✅ |
| `music.rewind()` | ✅ | `engine.music_rewind()` | ✅ |
| `music.stop()` | ✅ | `engine.music_stop()` | ✅ |
| `music.pause()` | ✅ | `engine.music_pause()` | ✅ |
| `music.unpause()` | ✅ | `engine.music_resume()` | ✅ |
| `music.fadeout(ms)` | ✅ | `engine.music_fade_out(ms)` | ✅ |
| `music.set_volume(v)` | ✅ | `engine.music_set_volume(v)` | ✅ |
| `music.get_volume()` | ✅ | `engine.music_get_volume()` | ✅ |
| `music.get_busy()` | ✅ | `engine.music_is_playing()` | ✅ |
| `music.set_pos(pos)` | ✅ | `engine.music_seek(pos)` | ✅ |
| `music.get_pos()` | ✅ | `engine.music_position()` | ✅ |
| `music.queue(file)` | 🟡 | `engine.music_queue(data)` | ✅ 但只能排队一个（pygame 允许多个排队） |
| `music.set_endevent()` / `get_endevent()` | ❌ | — | 缺播放结束事件通知 |

**Music 的覆盖度已经很高了**，主要缺的是事件通知和多队列排队。

---

## 五、总结：你必须补的 vs 可以缓一缓的

### 优先级 P0 — 必须现在就补

| 缺失 | 原因 |
|---|---|
| **WAV 解码器** | 没有解码器就无法测试任何一个播放功能 |
| **`maxtime` 参数** | `Sound.play(loops, maxtime, fade_ms)` 缺一个参数不对称 |
| **`SfxChannel.get_sound()` 引用** | 没有它用户不知道正在播什么 |

### 优先级 P1 — 建议补

| 缺失 | 原因 |
|---|---|
| **独立 L/R 声道音量** | `set_volume(left, right)` 是 pygame 的标准接口 |
| **全局 `fadeout()`** | `mixer.fadeout()` 是 pygame 标准 API |
| **声道级 `queue()`** | 排队长尾 SFX 的刚需 |
| **音量读取** | `.get_volume()` 缺缺少可对称 |

### 优先级 P2 — 可以缓一缓

| 缺失 | 原因 |
|---|---|
| **`get_raw()`** | 调试和特殊效果用，不影响核心播放 |
| **多队列排队** | 先实现单排队，需要时再扩 |
| **`get_endevent()`** | 需要事件系统的整合，Phase 4~5 做 |
| **运行时动态调整声道数** | 创建时指定就够用了 |

---

## 六、原型已有 vs pygame 的整体覆盖度

```
pygame.mixer（顶层）   9/10  ✅
pygame.mixer.Sound    6/9   🟡  缺 maxtime/get_num_channels/get_raw
pygame.mixer.Channel  6/12  🟡  缺 L/R 音量/排队/结束事件/get_sound
pygame.mixer.music    13/15 ✅  接近全覆盖

整体覆盖：~75%
核心播放链路覆盖：~90%
```
