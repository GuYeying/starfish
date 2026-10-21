# SDL Audio 运行时原理 — 从底层理解音频播放

> 目标读者：熟悉 wgpu 渲染管线但对 SDL 音频一无所知的引擎开发者。
> 本文用你熟悉的渲染概念作类比，解释 SDL 音频的整个运行时流水线。

---

## 1. 宏观类比：渲染 vs 音频

| 渲染 (wgpu) | 音频 (SDL) |
|---|---|
| GPU | 声卡（DAC + 扬声器） |
| Swapchain | 音频设备环形缓冲区 |
| V-Sync 每 16ms 刷新一次 | 音频设备以固定频率请求数据 |
| `begin_frame()` → 绘制 → `present()` | `callback()` → 填充数据 → 驱动取走 |
| 帧率 ~60fps | 回调频率 ~10~60 次/秒（取决于缓冲区大小） |
| CommandBuffer 提交队列 | AudioStream 数据管道 |
| 分辨率 1920×1080 | 采样率 44100 Hz，声道 2 |
| 每个像素 RGBA 4 字节 | 每个采样 f32 4 字节，立体声每帧 8 字节 |

**核心差异**：渲染是"推"模式（你提交命令，GPU 执行）；音频是"拉"模式（声卡说"给我数据"，你填充）。

---

## 2. SDL 音频设备的生命周期

```
┌─────────────────────────────────────────────────────────────┐
│                    App 启动                                   │
├─────────────────────────────────────────────────────────────┤
│  1. SDL_Init(SDL_INIT_AUDIO)                                 │
│     → 初始化音频子系统，加载后端驱动 (WASAPI/ALSA/PulseAudio)  │
├─────────────────────────────────────────────────────────────┤
│  2. SDL_OpenAudioDevice(device_id, &spec)                    │
│     → 打开音频设备                                           │
│     → SDL 创建一个内部音频线程                                │
│     → 线程循环：等待 → 回调 → 等待                            │
├─────────────────────────────────────────────────────────────┤
│  3. callback(void* userdata, Uint8* stream, int len)         │
│     → 音频线程调用你的回调函数                                 │
│     → 你把 len 字节的 PCM 数据写入 stream                      │
│     → 返回后，SDL 把 stream 内容送入驱动 → 声卡播放           │
├─────────────────────────────────────────────────────────────┤
│  4. SDL_CloseAudioDevice()                                   │
│     → 停止音频线程                                            │
│     → 释放设备                                                │
└─────────────────────────────────────────────────────────────┘
```

### SDL3 的新变化：AudioStream

SDL3 引入了 `AudioStream`，替代了 SDL2 的直接回调风格：

```
SDL2 风格（纯回调）：
  callback(userdata, stream, len) {
      // 你必须在这里同步填充 stream
      memcpy(stream, my_data, len);
  }

SDL3 风格（AudioStream，推荐）：
  callback(userdata, stream, additional_amount) {
      // stream 是一个 AudioStream 对象
      // 你用 put_data 往里推数据
      SDL_PutAudioStreamData(stream, my_data, my_len);
      // SDL 自动做格式转换 + 重采样
  }
```

你的项目用的是 SDL3 风格（`AudioStreamWithCallback`），这是正确的选择。

---

## 3. 音频线程模型 —— 关键理解

SDL 音频 API 最需要理解的一点：**回调在独立的音频线程中执行**。

```
                    ┌─────────────────────┐
                    │    主线程 (main)      │
                    │  游戏循环             │
                    │  while(running) {     │
                    │    处理事件            │
                    │    更新逻辑            │
                    │    渲染                │
                    │  }                   │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │    音频线程 (高优先级) │
                    │                      │
                    │  while(device_open) { │
                    │    睡眠直到需要数据    │
                    │    → 回调函数()       │
                    │    → 驱动取走数据      │
                    │  }                   │
                    └──────────────────────┘
```

### 线程安全注意事项

这是你整个 mixer 设计中**最需要重视的地方**：

| 操作 | 线程 | 说明 |
|---|---|---|
| `play_sound()` | 主线程 | 分配 channel，启动播放 |
| `stop()` / `set_volume()` | 主线程 | 修改 channel 状态 |
| `on_frames()` → 读 channel 数据 | **音频线程** | **每帧被调用** |
| `channel.read_frames()` | 音频线程 | 读取 PCM，推进 cursor |

因此，主线程和音频线程会**并发访问 channel 列表**。你需要：

- **`Channel` 的状态字段用 `Atomic`**（`play_state: AtomicBool`）
- **Channel 列表用 `Mutex` 或 `RwLock`**
- 或者使用**双缓冲 channel 列表**（mixer 内部 swap 一份快照给音频线程）

> ⚠️ **不要在音频回调中加锁等待主线程**——这会导致音频卡顿（underrun）。音频回调应该无阻塞。
> 最佳实践：用一个无锁 SPSC 队列（如 `crossbeam::channel`）从主线程向音频线程发命令。

---

## 4. 回调触发频率 —— 缓冲区大小决定一切

音频设备以固定 `spec.freq`（采样率）工作，每次回调的数据量由 `spec.samples` 决定：

```
回调频率 = 采样率 / samples  (每秒回调次数)
每次回调的字节数 = samples * 声道数 * 每个采样字节数

示例 1（低延迟，高频回调）：
  采样率: 44100 Hz
  samples: 1024
  回调频率: 44100 / 1024 ≈ 43 次/秒
  每次数据量: 1024 * 2(f32立体声) * 4字节 = 8192 字节
  每帧持续时间: 1024 / 44100 ≈ 23ms

示例 2（高吞吐，低频回调）：
  采样率: 48000 Hz
  samples: 4096
  回调频率: 48000 / 4096 ≈ 11.7 次/秒
  每次数据量: 4096 * 2 * 4 = 32768 字节
  每帧持续时间: 4096 / 48000 ≈ 85ms
```

### 在 Mixer 中的含义

你的回调每次需要生成 **N 个 StereoFrame**（N = `additional_amount / 2`）：
- 小缓冲区（samples=512~1024）：延迟低 (~10~23ms)，CPU 友好
- 大缓冲区（samples=2048~4096）：更稳定，但响应延迟高

**建议 mixer 默认使用 1024 或 2048 samples**，这是游戏音频的黄金区间。

---

## 5. 完整的音频数据流

```
                         ┌──────────────────┐
  硬盘上的音频文件        │  decoder.rs       │
  (sound.wav)            │  hound / symphonia │
      │                  └────────┬─────────┘
      ▼                           ▼
  PCM 数据               ┌──────────────────┐
  (原始采样值)            │  SoundData         │
  WAV: 44100Hz, 16bit    │  frames: Vec<StereoFrame<f32>> │
       mono              └────────┬─────────┘
      │                           │
      ▼                           ▼
  Channel                   ┌──────────────────┐
  (带音量/pan/fade)         │  channel.read_frames() │
      │                     │  → 按 cursor 位置取     │
      │                     │  → 应用 volume/pan      │
      │                     │  → 推进 cursor          │
      ▼                     └────────┬─────────┘
                            ┌──────────────────┐
  所有 Channel 混合          │  mixer.on_frames() │
  (帧对齐求和)               │  → 遍历 channels    │
      │                     │  → 帧对齐叠加        │
      ▼                     │  → clamp(-1, 1)     │
                            └────────┬─────────┘
                            ┌──────────────────┐
  frames_to_samples          │  PlaybackCallback  │
  (StereoFrame → f32[])     │  → 转换格式         │
      │                     │  → put_data_f32()   │
      ▼                     └────────┬─────────┘
                            ┌──────────────────┐
  SDL AudioStream            │  自动重采样 + 声道转换 │
  (内部管理环形缓冲区)       └────────┬─────────┘
      │                             │
      ▼                             ▼
  音频驱动缓冲区             WASAPI/ALSA/PulseAudio
  (硬件环形缓冲区)            → 按中断频率取数据
      │
      ▼
  DAC (数模转换器)
      │
      ▼
  扬声器
```

---

## 6. 深入：回调内到底发生了什么？

这是当 `PlaybackCallback::callback(stream, additional_amount)` 被调用时，完整的内部流程：

```rust
fn callback(&mut self, stream: &mut AudioStream, additional_amount: i32) {
    // Step 1: 计算本次需要多少帧
    // additional_amount 是 SDL 请求的字节数，但这是格式转换前的估计值
    // 实际要多少帧取决于音频线程的定时
    let sample_count = (additional_amount as usize).min(MAX_SAMPLES);
    let frame_count = sample_count / 2;

    if frame_count == 0 {
        return;  // 无需处理
    }

    // Step 2: 清空栈上帧缓冲区
    let frames = &mut self.frame_buffer[..frame_count];
    frames.fill(StereoFrame::SILENT);

    // Step 3: 调用用户（Mixer）的帧生成函数
    // ⚠️ 这里运行在音频线程！
    self.user_cb.on_frames(frames);
    // frames 现在包含 mix 后的音频数据

    // Step 4: 将 StereoFrame 转换为 f32 交错采样
    let samples = &mut self.sample_buffer[..frame_count * 2];
    if let Err(e) = frames_to_samples(frames, samples) {
        return;
    }

    // Step 5: 送入 SDL AudioStream
    // SDL 内部再送入硬件缓冲区
    let _ = stream.put_data_f32(samples);
}
```

### SDL AudioStream 内部发生了什么？

当你调用 `stream.put_data_f32(samples)`：

```
你输入: [f32; N] 交错立体声, 44100Hz
                        │
                        ▼
              ┌─────────────────┐
              │  AudioStream      │
              │                   │
              │  1. 格式转换       │
              │     f32 → f32(s)  │
              │     (你输入f32,    │
              │      设备也是f32,  │
              │      所以跳过)     │
              │                   │
              │  2. 重采样         │
              │     44100 → 48000 │
              │     (如果设备是    │
              │      48kHz)       │
              │                   │
              │  3. 声道转换       │
              │     stereo→stereo │
              │     (跳过)         │
              │                   │
              │  4. 存入环形缓冲区  │
              │     等待驱动取走    │
              └─────────────────┘
                        │
                        ▼
              音频驱动读取
              送入 DAC → 扬声器
```

**关键点**：`additional_amount` 是 SDL 估计的、经过格式转换前的字节数。你的回调不一定正好填这个数——少填会造成轻微的停顿（underrun），多填会延迟播放。

---

## 7. Mixer 中的时间线：一个声音从 play 到结束

```
主线程:                          音频线程 (回调):
                                    │
mixer.play(sound_data)               │
  └→ 找到空闲 channel                │
  └→ channel.reset(sound)           │
  └→ channel.state = Playing        │
  └→ channel.cursor = 0             │
      │                             │
      ▼                             │
                                    ▼
                            on_frames() 被调用
                              └→ for each playing channel:
                                    channel.read_frames(buf)
                                      └→ buf[0..N] = sound.frames[cursor..cursor+N]
                                      └→ cursor += N
                                      └→ if cursor >= total:
                                            if looping: cursor = loop_start
                                            else: state = Stopped
                              └→ mix all buf → output
                                    
                                    ▼
                            on_frames() 再次调用
                              └→ ...重复直到 channel Stopped

主线程:                           音频线程:
mixer.set_volume(ch, 0.5)          │
  └→ channel.volume = 0.5         │
      │             (atomic set)   │
      ▼            下一个回调自动读取新值
                                    ▼
                            on_frames() 使用新音量值
```

---

## 8. 关于延迟（Latency）的直觉

```
音频数据的"旅行时间"：

┌──────┐    ┌──────────┐    ┌──────────┐    ┌──────┐
│ 应用  │───→│ SDL 环形  │───→│ 驱动缓冲  │───→│ DAC  │
│ 回调  │    │ 缓冲区    │    │ 区       │    │      │
└──────┘    └──────────┘    └──────────┘    └──────┘
  ~0ms        samples/rate       ~10ms         ~1ms
               ≈23ms @1024
               ≈5.8ms @256

总延迟 ≈ (samples / 采样率) * 2 + 驱动延迟 + DAC 延迟
       ≈ 23*2 + 10 + 1 ≈ 57ms  (1024 samples)
       ≈ 5.8*2 + 10 + 1 ≈ 22ms (256 samples)
```

**游戏通常接受 50~100ms 的音频延迟**，比视频的 16ms 宽松得多。SDL 默认的 1024~2048 samples 足够了，不需要追求极低延迟。

---

## 9. 你现在需要做什么：实操清单

### Step 1：添加音频解码器

```toml
# Cargo.toml 追加
hound = "3.5"  # WAV 起步
```

### Step 2：创建 `src/base/audio/` 骨架

```
src/base/audio/
├── mod.rs              # 重导出
├── decoder/
│   ├── mod.rs          # Decode trait
│   └── wav.rs          # hound → SoundData
└── mixer/
    ├── mod.rs          # Mixer struct
    ├── channel.rs      # Channel
    ├── track.rs        # SoundData
    └── effect.rs       # Fade/Pan/Volume
```

### Step 3：实现核心混音回调

在 mixer 的 `AudioUserCallback::on_frames()` 中实现多通道叠加（第 4 节伪代码）。

### Step 4：注意线程安全

- `Channel.state` → `AtomicBool`
- `Channel.volume` → `AtomicU32`（编码为整数）
- Channel 列表 → `Mutex<Vec<Channel>>` 或 `RwLock`
- 或者在音频回调中只读快照（主线程修改后原子 swap）

### Step 5：暴露给 `base/mod.rs`

```rust
// base/mod.rs 追加
pub mod audio;
```

---

## 10. 参考：SDL3 音频 API 速查

| 函数 | 作用 | Mixer 中使用场景 |
|---|---|---|
| `SDL_GetAudioPlaybackDevices()` | 枚举播放设备 | mixer 初始化时选择设备 |
| `SDL_OpenAudioDevice()` | 打开设备 | `Mixer::open()` |
| `SDL_CreateAudioStream()` | 创建格式转换流 | 音频流重采样 |
| `SDL_PutAudioStreamData()` | 向流中推数据 | `PlaybackCallback` 中调用 |
| `SDL_GetAudioStreamData()` | 从流中取转换后数据 | 录音时使用 |
| `SDL_GetAudioStreamQueued()` | 查询流中排队数据量 | 同步/延迟查询 |
| `SDL_GetAudioDeviceFormat()` | 查询设备实际格式 | mixer 获取输出采样率 |
| `SDL_GetAudioDriver()` | 查询当前音频后端 | 调试信息 |
| `SDL_PauseAudioDevice()` | 暂停/恢复设备 | `mixer.pause_all()` / `resume_all()` |

---

> **一句话总结**：SDL 音频 = 一个独立的高优先级线程，定期调你的回调函数填数据。Mixer 的核心就是在回调里把多个 Channel 的 PCM 帧叠在一起输出。线程安全是唯一的坑，用原子操作和无锁队列就能绕过去。
