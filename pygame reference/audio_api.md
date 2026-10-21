# Audio 音频引擎

## 架构总览

```
src/base/audio/
├── mod.rs              ← AudioEngine 主入口（全部公开 API）
├── sfx.rs              ← SfxChannel + AudioEffect trait + FadeState
├── music.rs            ← MusicPlayer（背景音乐播放器）
├── track.rs            ← SoundData（解码后的 PCM 数据容器）
└── decoder/
    ├── mod.rs
    └── symphonia.rs    ← SymphoniaDecoder（WAV/OGG/MP3/FLAC 解码）
```

### 运行时架构

```
主线程（游戏逻辑）                   音频线程（SDL 回调）
─────────────────                   ──────────────────
AudioEngine                          PlaybackCallback::callback()
  ├── play(sound)                          ↓
  ├── stop()                     AudioEngineCallback::on_frames()
  ├── set_volume()                       ↓
  └── ...                       Inner::mix(output)
                                  ├── Vec<SfxChannel> 遍历叠加
                                  └── MusicPlayer     叠加
                                       ↓
                                frames → put_data → SDL → 声卡
```

线程安全模型：
- `Inner` 通过 `Arc<Mutex<>>` 保护（主线程写、音频线程读写）
- 三个音量（master/sfx/music）用 `AtomicU32`（编码 f32 bit pattern），无锁读取
- 音频回调中持锁时间 ≈ 遍历 channel + 浮点叠加，微秒级

---

## AudioEngine — 主 API

### 创建

```rust
pub fn new(subsystem: &AudioSubsystem, sample_rate: u32, num_channels: u32)
    -> Result<Self, AudioError>
```

- `sample_rate`: 输出采样率（Hz），如 44100、48000
- `num_channels`: SFX 声道数（建议 8~16）
- 内部固定使用 F32LE 立体声，SDL3 的 AudioStream 自动处理设备格式转换

高级用法（自定义 AudioSpec）：
```rust
pub fn new_with_spec(subsystem: &AudioSubsystem, spec: &AudioSpec, num_channels: u32)
    -> Result<Self, AudioError>
```

### 公开字段

```rust
pub output_sample_rate: u32
```

当前音频设备的实际输出采样率。加载音频文件后，如果文件采样率与此不同，用 `sound_data.resample(self.output_sample_rate)` 转换。

---

### SFX 播放

#### play — 自动分配声道

```rust
pub fn play(&mut self, sound: Arc<SoundData>) -> Result<Option<usize>, AudioError>
```

**参数**:
- `sound`: `Arc<SoundData>` — 解码后的 PCM 数据（允许 clone 共享，零重复解码）

**返回**:
- `Ok(Some(ch))` — 正在声道 `ch` 播放
- `Ok(None)` — 所有声道繁忙，未播放（不是错误）
- `Err(e)` — 实际错误（mutex 中毒等）

**示例**:
```rust
match engine.play(gunshot.clone())? {
    Some(ch) => println!("声道 {} 响起枪声", ch),
    None => println!("太吵了，等会儿再开枪"),
}
```

#### play_with — 完整参数播放

```rust
pub fn play_with(&mut self, sound: Arc<SoundData>, loops: i32, fade_in_ms: f32)
    -> Result<Option<usize>, AudioError>
```

**参数**:
- `loops`: `i32`
  - `-1` = 无限循环
  - `0` = 播一次
  - `N` (N>0) = 播 N+1 次
- `fade_in_ms`: `f32` — 淡入时长（毫秒），`0.0` = 无淡入

**示例**:
```rust
// 播放枪声，淡入 100ms
engine.play_with(gunshot, 0, 100.0)?;

// 无限循环播放引擎轰鸣
engine.play_with(engine_rumble, -1, 500.0)?;
```

#### play_on — 指定声道播放

```rust
pub fn play_on(&mut self, sound: Arc<SoundData>, channel: usize) -> Result<(), AudioError>
```

**参数**:
- `channel`: `usize` — 目标声道索引（0 ~ num_channels-1）

#### play_in_group — 指定分组播放

```rust
pub fn play_in_group(&mut self, group: Option<GroupHandle>, sound: Arc<SoundData>,
    loops: i32, fade_in_ms: f32) -> Result<Option<usize>, AudioError>
```

**参数**:
- `group`: `Option<GroupHandle>`
  - `Some(handle)` = 只在该分组中找空闲声道
  - `None` = 在所有非保留声道中找

---

### SFX 控制

#### stop / stop_all

```rust
pub fn stop(&mut self, channel: usize)
pub fn stop_all(&mut self)
```

#### pause / resume

```rust
pub fn pause(&mut self, channel: usize)
pub fn resume(&mut self, channel: usize)
pub fn pause_all(&mut self)
pub fn resume_all(&mut self)
```

#### 音量 / 声像

```rust
pub fn set_channel_volume(&mut self, channel: usize, volume: f32)
```

- `volume`: `f32` — `0.0`(静音) ~ `1.0`(最大)

```rust
pub fn set_channel_pan(&mut self, channel: usize, pan: f32)
```

- `pan`: `f32` — `-1.0`(全左) ~ `0.0`(居中) ~ `1.0`(全右)

#### 淡入 / 淡出

```rust
pub fn channel_fade_in(&mut self, channel: usize, ms: u32)
pub fn channel_fade_out(&mut self, channel: usize, ms: u32)
```

- `ms`: 淡变持续时间（毫秒），**不是延迟多久后开始**
- `fade_in`: 音量 `0 → 1`，完成后自动清除淡变状态
- `fade_out`: 音量 `1 → 0`，完成后自动 `stop`

#### 查询

```rust
pub fn is_channel_busy(&self, channel: usize) -> bool
pub fn get_channel_sound(&self, channel: usize) -> Option<Arc<SoundData>>
pub fn get_busy(&self) -> bool                    // 任何声道或 BGM 在播放？
pub fn find_free_channel(&self) -> Option<usize>
pub fn num_channels(&self) -> usize
pub fn set_reserved(&mut self, n: usize)          // 保留前 N 个声道不被自动分配
```

---

### 分组管理（Group）

分组是不透明句柄，通过 `create_group()` 创建。用于批量管理一组声道。

#### 创建与分配

```rust
pub fn create_group(&mut self) -> GroupHandle

pub fn set_channel_group(&mut self, channel: usize, group: GroupHandle)
    -> Result<(), AudioError>

pub fn set_channel_group_range(&mut self, from: usize, to: usize,
    group: Option<GroupHandle>) -> Result<(), AudioError>

pub fn remove_channel_group(&mut self, channel: usize) -> Result<(), AudioError>
```

**示例**:
```rust
let sfx_group = engine.create_group();
engine.set_channel_group_range(0, 5, Some(sfx_group))?;
engine.remove_channel_group(3)?;  // 声道 3 移出分组
```

#### 批量控制

```rust
pub fn stop_group(&mut self, group: GroupHandle)
pub fn fade_out_group(&mut self, group: GroupHandle, ms: u32)
```

#### 查询

```rust
pub fn group_count(&self, group: GroupHandle) -> usize
pub fn group_busy(&self, group: GroupHandle) -> bool
pub fn group_available(&self, group: GroupHandle) -> Option<usize>
```

---

### 效果器链

#### 为 SFX 声道添加效果器

```rust
pub fn sfx_add_effect(&mut self, channel: usize, effect: Box<dyn AudioEffect>)
    -> Result<(), AudioError>
```

#### 为 BGM 添加效果器

```rust
pub fn music_add_effect(&mut self, effect: Box<dyn AudioEffect>)
```

效果器按添加顺序串联处理：`read_frames → effects[0] → effects[1] → ... → 淡变 → mix`

---

### 全局音量（三层模型）

```
最终输出 = master_volume * (sfx_volume * channel_volume + music_volume)
```

```rust
pub fn set_master_volume(&self, volume: f32)     // 0.0 ~ 1.0
pub fn master_volume(&self) -> f32

pub fn set_sfx_volume(&self, volume: f32)         // SFX 分组音量
pub fn sfx_volume(&self) -> f32
```

---

### Music API

```rust
pub fn music_load(&mut self, data: Arc<SoundData>)       // 加载
pub fn music_play(&mut self, loops: i32)                  // loops: -1=无限, 0=一次
pub fn music_stop(&mut self)
pub fn music_pause(&mut self)
pub fn music_resume(&mut self)
pub fn music_fade_in(&mut self, ms: u32)
pub fn music_fade_out(&mut self, ms: u32)
pub fn music_set_volume(&self, volume: f32)
pub fn music_get_volume(&self) -> f32
pub fn music_seek(&mut self, seconds: f32)                // 跳转
pub fn music_position(&self) -> f32                        // 当前秒数
pub fn music_duration(&self) -> f32                        // 总时长
pub fn music_is_playing(&self) -> bool
pub fn music_queue(&mut self, data: Arc<SoundData>)       // 排队下一首
pub fn music_rewind(&mut self)
```

---

## AudioEffect — 效果器 Trait

所有效果器必须实现此 trait：

```rust
pub trait AudioEffect: Send + 'static {
    /// 效果器名称（调试用）
    fn name(&self) -> &str;

    /// 处理一段 PCM 帧数据（直接修改 frames 中的值）
    fn process(&mut self, frames: &mut [StereoFrame]);

    /// 声道停止时调用（可选，用于清理效果器内部状态）
    fn on_channel_stop(&mut self) {}
}
```

### 实现示例

```rust
use starfish::base::audio::sfx::AudioEffect;
use starfish::base::subsystem::audio::common::StereoFrame;

struct Distortion { drive: f32 }

impl AudioEffect for Distortion {
    fn name(&self) -> &str { "distortion" }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        for f in frames {
            f.left  = (f.left * self.drive).tanh();
            f.right = (f.right * self.drive).tanh();
        }
    }
}

// 使用
engine.sfx_add_effect(0, Box::new(Distortion { drive: 2.0 }))?;
```

效果器会在音频线程中调用，**不要在 process 中做堆分配或耗时操作**。

---

## SoundData — 解码后的 PCM 数据

```rust
pub struct SoundData {
    pub frames: Vec<StereoFrame>,
    pub sample_rate: u32,
}
```

### 方法

```rust
pub fn from_interleaved_f32(samples: &[f32], sample_rate: u32) -> Self
pub fn from_mono_f32(samples: &[f32], sample_rate: u32) -> Self
pub fn duration(&self) -> f32
pub fn frame_count(&self) -> usize
pub fn sample_count(&self) -> usize
pub fn resample(&self, target_sample_rate: u32) -> Self
```

`resample()` 使用线性插值，同一性（sample_rate == target）时零拷贝 clone。

---

## SfxChannel — 单个声道

```rust
pub struct SfxChannel {
    pub state: ChannelState,                   // Playing / Paused / Stopped
    pub data: Option<Arc<SoundData>>,          // 正在播放的音频
    pub cursor: usize,                         // 当前帧位置
    pub loops: i32,                            // -1=无限 / 0=一次 / N=N+1次
    pub volume: f32,                           // 0.0 ~ 1.0
    pub pan: f32,                              // -1.0(左) ~ 0.0(中) ~ 1.0(右)
    pub fade: Option<FadeState>,               // 淡变状态
    pub effects: Vec<Box<dyn AudioEffect>>,    // 效果器链
    pub group: Option<GroupHandle>,            // 所属分组
}
```

直接操作 `SfxChannel` 的字段（需要锁 `Inner`），但通常通过 `AudioEngine` 的公开方法操作。

---

## FadeState — 淡变状态机

```rust
pub struct FadeState {
    pub fade_type: FadeType,   // In / Out
    pub elapsed: usize,        // 已消耗帧数
    pub total: usize,          // 总淡变帧数
}
```

基于帧数计算，不依赖时间，音频线程中无锁无分配。ms 转帧数 = `ms * sample_rate / 1000`。

---

## Decoder — 解码器

```rust
use starfish::base::audio::decoder::SymphoniaDecoder;

// 自动识别格式（WAV / OGG / MP3 / FLAC）
let sound = SymphoniaDecoder::from_file("shoot.wav")?;
let music = SymphoniaDecoder::from_file("theme.ogg")?;
```

---

## 完整使用示例

```rust
use std::sync::Arc;
use starfish::base::audio::{AudioEngine, GroupHandle, decoder::SymphoniaDecoder};
use starfish::base::subsystem::AudioSubsystem;

fn main() -> Result<(), starfish::base::subsystem::audio::common::AudioError> {
    let sdl = sdl3::init().unwrap();
    let audio = AudioSubsystem::new(&sdl);

    // 1. 创建引擎（44100Hz, 8 个 SFX 声道）
    let mut engine = AudioEngine::new(&audio, 44100, 8)?;

    // 2. 解码
    let sfx  = Arc::new(SymphoniaDecoder::from_file("shot.wav")?);
    let bgm  = Arc::new(SymphoniaDecoder::from_file("bgm.ogg")?);

    // 重采样到输出采样率
    let sfx = if sfx.sample_rate != engine.output_sample_rate {
        Arc::new(sfx.resample(engine.output_sample_rate))
    } else { sfx };
    let bgm = if bgm.sample_rate != engine.output_sample_rate {
        Arc::new(bgm.resample(engine.output_sample_rate))
    } else { bgm };

    // 3. 播放
    let ch = engine.play(sfx.clone())?.unwrap_or(0);
    engine.music_load(bgm);
    engine.music_play(-1);

    // 4. 控制
    engine.set_channel_volume(ch, 0.8);
    engine.channel_fade_in(ch, 200);
    engine.music_fade_in(2000);

    // 5. 分组
    let fx = engine.create_group();
    engine.set_channel_group(ch, fx)?;
    engine.stop_group(fx);

    // 6. 查询
    println!("BGM 位置: {:.1}/{:.1}s",
        engine.music_position(), engine.music_duration());

    Ok(())
}
```
