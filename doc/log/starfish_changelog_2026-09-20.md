# Starfish 更新日志 2026-09-20

> 本日主题：**video 音轨 ②③ 落地**（9-19 交接立项）——mp4_demux 音轨提取 +
> symphonia AAC 解码 + `open_with_audio` 挂 mixer 流式声部。画面各平台硬解、
> 音轨全平台 symphonia 软解，"六平台同一硬解链路"补上声学维度。设计稿标记的
> **ADTS 头风险经源码核实反转**：解码入口吃裸 GA 帧、不打 ADTS，落地比设计
> 更简（零包装零 esds 解析）。批次 8 为音轨落地后的用户审读问答——流式声部
> 多路语义 / 与 SFX 的资源模型分叉 / 三类声源选择判据，澄清沉淀无代码改动。

## 批次 1：测试资产核实（② 前置检查，按交接要求执行）

`resources/videos/sample-5s.mp4` 音轨"是否含 AAC 未验证过"（交接遗留疑点，
本机无 ffprobe）——Python 裸解析 box 结构定案：

- 两轨：`vide`（avc1，timescale 15360）+ **`soun`（mp4a，timescale 44100）**
- esds → DecoderConfigDescriptor objectTypeIndication `0x40`（MPEG-4 AAC）
  → DecSpecificInfo ASC `12 10`：**AOT=2（AAC-LC）/ 44100Hz / 立体声**，
  ~128kbps，5.76s
- **无需转码**，现有资产直接可用；且正好命中"44100 ≠ 设备混音域"的重采样
  设计场景。全轨解码实测：开头 ~370ms 为静音引导段（首轮只测前 16 帧曾误判
  全零，全轨判读后排除）

## 批次 2：mp4_demux 音轨提取（②）

- **模块门控重构**：原 `#![cfg(any(android, wasm32, test))]` 整体门控拆除，
  按条目分家——视频轨 `Demuxer`（android/web 后端专用，桌面自解复用）保持
  原门控；音轨 `AudioDemuxer` **全平台参与编译**（音频无平台硬解承诺，
  桌面同样走 mp4 crate + symphonia；源文件双开 = 既定架构，桌面视频后端本
  就与未来音轨泵各开一次文件）
- `AudioDemuxer<R: Read + Seek>`：找 `TrackType::Audio` + `MediaType::AAC`
  轨 → `sample_freq_index().freq()` / `channel_config() as u8` 取元数据
  （声道越界（0 或 >2）报错，守住 AAC-LC 立体声契约）→
  `next_sample()` 拉裸 AAC 样本（`Ok(None)` = 音轨结束）
- `AudioTrackInfo { sample_rate, channels }`——解码器装配参数的事实源

## 批次 3：AAC 解码（②）——ADTS 风险核实反转

交接设计："mp4 里是 raw AAC（无 ADTS 头），symphonia 的 AAC 解码入口期望
ADTS——成熟做法是手动打 7 字节 ADTS 头再喂解码器"。**读 symphonia-codec-aac
0.5.5 源码核实：前提不成立**——`AacDecoder::decode_inner` 直接
`BitReaderLtr::new(packet.buf())` 进 `decode_ga`，**根本不解析 ADTS 头**
（打了 ADTS 反而会把头字节当音频载荷解码失败）。源码同时给出官方无 ASC
路径：`try_new` 无 `extra_data` 时"assume no ASC and use the codec
parameters"——LC/1024 帧/采样率/声道全部从 `CodecParameters` 取。

**落地（比设计稿更简）**：

- symphonia features 加 `"aac"`（mp4 音轨硬链路）
- `AudioDemuxer` 的裸样本直接 `Packet::new_from_boxed_slice` 喂解码器——
  **零 ADTS 包装、零 esds/ASC 解析**（mp4 crate 的
  sample_freq_index/channel_config 已给出全部装配参数，ADTS 方案需要的
  采样率/声道信息来源相同）
- 0.5 的 `CodecParameters` 是公开字段（无 builder setter），逐字段赋值；
  `Packet` 构造非 Result（ts/dur 不参与解码）
- 解码输出经 `SampleBuffer<f32>` 转交错 f32；单声道复制 L=R（混音环只收
  交错立体声）

## 批次 4：流式线性插值重采样（④ 地基，`base/audio/resample.rs` 新模块）

`SoundData::resample` 同款算法的流式版（线性插值，音质契约一致）：

- 状态跨块：`next_out`（源流绝对坐标）延续 + 上一块末帧作跨块插值左邻
  （右邻保证在当前块内）；`flush()` 复制末帧补齐尾流
- 输入交错立体声（单声道由调用方先复制，泵内完成）；输出目标采样率
- 测试锚定三条性质：恒值信号恒等、**分块与整段输出严格一致**（奇数长度
  块破坏帧对齐属调用方契约，测试注释言明）、flush 后总帧数 ≈ 时长×目标率

## 批次 5：`open_with_audio` 集成（③）

- **`base/video/audio_track.rs`（新）——音轨泵**：`AudioDemuxer` 拉裸 AAC
  → symphonia 逐包解码 →（源率 ≠ 声部率时）`StreamResampler` →
  `StreamVoice::push_interleaved`。驱动点在 `Video::update`（共用核心不
  假设线程存在，桌面/Web 同一份代码）
- **同步纪律（v1 = 视频时钟）**：`pushed`（已解码音轨位置，源时钟）落后于
  视频主时钟才继续解码；**背压满即停推**，剩余留 `pending` 下帧优先冲销，
  绝不丢弃；**首帧对齐**——泵首次被泵时 `pushed = clock`（中途打开不回放
  历史，"同帧起播近似同步"由此成立）
- **容错**：单帧解码失败跳过，连续 32 帧失败才禁用泵（console_log 诊断）
  ——坏音轨不拖死画面
- **平台分叉收敛**：native `open` 同步装配（读文件 + demux + 解码器，失败
  即 Err 可感知）；web `open` 异步 fetch 同一 URL（`web::fetch_bytes` 提取
  共用，视频后端内联 fetch 换用之），装配失败仅静音降级（web 无同步错误
  通道）。推帧逻辑经 `with_core` 收敛为一份（`Option` 直取 / RefCell 借用
  的形态差吸收在两个 6 行 cfg 适配器里）
- **`Video`**：新 `audio: Option<AudioPump>` 字段 + `with_audio` 构造；
  `update` 在时钟推进后泵音轨（不受 `set_video_enabled` 遮挡开关影响——
  遮挡语义本就是"只走音频/冻结画面"）；控制面 `set_audio_volume` /
  `set_muted` / `has_audio` 直通声部
- **`VideoModule`**：`open_backend` 提炼（open / open_with_audio 共用平台
  分发）；`open_with_audio(path, voice)` 按设计稿签名落地

## 批次 6：StreamVoice 配套（顺带两处既有缺口）

- `StreamVoiceSlot` 增 `sample_rate`（`open_stream_voice` 注入混音域）+
  `pushed_frames` 计数；`StreamVoice` 增 `sample_rate()` / `pushed_frames()`
  ——泵的重采样目标与探针判读锚点（设计稿"推帧按 output_sample_rate"的
  声部侧落地：此前推方需要混音域采样率却无处可查）
- **补 `#[derive(Clone)]`**：文档写明"句柄 Clone 共享同一声部"但结构体漏了
  derive——补齐履行文档契约（probe_video 也需要留判读句柄）
- **fade 硬编码 48kHz 修正**：`fade_in/fade_out_and_close` 的
  `FadeState::new_*(ms, 48_000)` 改用 `slot.sample_rate`——非 48k 设备上
  淡变时长恒偏 ~8.8%，属声部化时遗留的潜在 bug，顺手根治

## 批次 7：probe_video 音轨相位 + 回归

- probe_video 升级：`open_with_audio` 直挂（mixer 帧内创建 + 声部句柄
  clone 留底）；**`AUDIO PASS frames=N rate=R`** 判读锚点——声部收到推帧
  即判（真机可听为伴生效果，无头以计数判定，同 record 的环境容差思路）；
  ended 时 `voice.fade_out_and_close(200)` 示范标准收尾
- CLAUDE.md probe 表 probe_video 行同步（OPEN/FIRST/AUDIO/ENDED）

### 测试状态（全部实测）

- `cargo test --lib`：**64 passed / 0 failed**（新增 8：demux 音轨 1 +
  AAC 解码 1 + 推泵 2 + 重采样 4）
  - 交接三判据全绿：demux 音轨样本数 > 0；AAC 解码 PCM 非全零（全轨判读，
    资产开头静音段曾致误判）；推入声部后 ring 增长（背压封顶 ring 容量 +
    整轨推完 ≈ 音轨时长×声部采样率）
- **桌面冒烟**：probe_video 全锚点 PASS，`audio_frames` 实时推进至 268864
  （≈5.6s × 48kHz 全轨），44100→48000 重采样在跑
- **wasm 无头存活模式**：`OPEN PASS +audio` / `AUDIO PASS frames=1114
  rate=48000`（异步 fetch→解码→推声部链路在 web 走通）/ `FIRST PASS
  1920x1080` → 实时推进 → `ENDED PASS`，audio_frames 271952
- 四目标 lib check 零 error：桌面 / aarch64-linux-android /
  aarch64-apple-ios / wasm32-unknown-unknown；`cargo check --examples` 全过；
  `--no-default-features` 与 `--features video` 单开均编译（音轨代码全部
  收敛在 video 特性门内，audio 侧 resample 模块恒参与）
- Android APK 出包：`cargo xtask android probe_video --build` 签名通过
  （`target/android-apk/probe_video_android.apk`；实机部署待用户）

### 经验沉淀

- **交接设计的技术风险要"读源码核实"而非"按设计执行"**：ADTS 风险是设计
  阶段的合理猜测，symphonia 源码 20 行定案反转——且无 ASC 路径让落地少写
  一个 ADTS 编码器 + 一个 esds 解析器
- **"句柄 Clone"这类文档契约**要在首次需要时回头核实（本例 derive 漏写，
  首个消费者 video 泵被迫绕路——好在 probe 阶段补齐）
- 时间类无头测试照例存活模式；资产开头静音段对"非全零"类判读是天然陷阱
  ——判据设计要么全轨扫，要么跳过引导段

---

## 批次 8：流式声部设计问答沉淀（用户审读驱动，音轨落地后复核）

> 用户审读 open_with_audio 时的三个追问：能否多路开、流式/非流式各是什么
> 设计、接口如何对偶。全部是对既有实现的澄清，无代码改动——但问答厘清的
> 资源模型与选择判据属于设计事实，按批次 6"审读即测试"惯例沉淀入册。

### ① 多路创建：每调一次开一路，互不干扰

`open_stream_voice()` 每次调用新建一个独立 `StreamVoiceSlot`（自己的环形
缓冲/音量/静音/淡变/关闭标记）push 进 `Inner.stream_voices: Vec`，混音回调
对每路读环求和。无数量上限，每路成本 ≈ 16384 帧 × 8B = **128KB 环形缓冲**
+ 混音循环一次读环——几十路毫无压力。

生命周期细节：`closed` 置位后由混音回调**惰性摘除**（`retain`）——淡出
（`fade_out_and_close`）走完才置位，期间数据继续消费防爆音；显式 `close`
立即置位，残留帧丢弃不播。无人推帧且不 close 的声部会一直留在 Vec（静音
空读开销 O(路数)，长期不用的声部应显式关闭）。

### ② 两套接口的资源模型刻意不同：池+槽位索引 vs 对象句柄

| | SFX（`play_with`） | 流式（`open_stream_voice`） |
|---|---|---|
| 资源模型 | **固定声道池**：`AudioMixer::new(n)` 预建 n 槽，播放 = 找空闲槽填充 | **随开随有**：每调一次新建一路 |
| 句柄形态 | `Option<usize>` 槽位索引（长期有效，播完自动回池复用） | `StreamVoice`（`Arc` 共享槽，可 Clone 多端持有） |
| 数据源 | 整段 `Arc<SoundData>`（一次性解码进内存） | 调用方推 PCM（推式，背压反压） |
| 生命周期 | **隐式**：播完 → `Stopped` 回池 | **显式三段式**：push → 控制 → close/fade_out_and_close |
| 独有面 | 分组总线（`GroupHandle`）、声像 pan、效果器链 | `set_muted`（视频静音要独立开关）、采样率契约 `sample_rate()` |

分叉的原因：SFX 是"数据一次到齐、播完即走"，池+抢占最简且对齐 pygame
`mixer.Channel` 语义；流式是"数据分批到、消费节奏拽着推方走"，每路必须
有**独立环形缓冲**和独立控制面，声部本身就是可共享对象。Clone 句柄是刚需
——probe_video 即两端正交操作同一路：`voice.clone()` 留底判读
`pushed_frames()`，`Video` 持原句柄推帧。

### ③ 接口对偶表（fade 参数退役的依据所在）

| 能力 | SFX 侧 | 流式侧 |
|---|---|---|
| 发声 | `play_with(sound, loops)` | `open_stream_voice()` + `push_interleaved` |
| 音量 | `set_channel_volume(idx, v)` | `voice.set_volume(v)` |
| 淡变 | `channel_fade_in/out(idx, ms)` | `voice.fade_in` / `fade_out_and_close(ms)` |
| 停止 | `stop(idx)` | `voice.close()` |
| 静音 | —（音量 0 代替） | `set_muted` |

批次 6⑨ play 家族 fade 参数退役的依据正是本表：先 play 再补一句
`channel_fade_in` 听感等价，淡变能力归通道层/声部层各自所有（对偶不重复）。

### ④ 三类声源分工与选择判据

mixer 三类声源（批次 6⑨ 定稿）各自的使用场景：

1. **SFX 声部**（非流式）——数据一次到位的短声音：枪声/跳跃/UI 点击；
   同一音效反复播（`Arc<SoundData>` 共享数据，只抢声道）；循环的静态
   环境音（`loops = -1`）；要分组统一调音量的类别
2. **流式声部**——数据**不是**（或不能）一次到位的：视频音轨（解码节奏
   被消费端反压，即 A/V 同步的物理锚点）、程序化合成（逐帧生成）、网络
   音频流；多路独立推入的动态声源
3. **music**（文件驱动流）——完整文件但太长不想整段进内存的 BGM，
   mixer 自带解码线程，应用零推帧

一句话判据：**拿得到完整缓冲且短 → SFX；拿得到完整文件但很长 → music；
数据边生产边消费 → 流式声部**。

---

## 待办与边界（下一会话参考）

- ④ A/V 精确同步（锚定声部 ring 读点）未动，v1 = 视频时钟近似同步
- web 音轨泵与视频后端各 fetch 一次同 URL（桌面同构：各开一次文件）——
  字节范围/共享下载留待实际带宽成为问题时
- probe_video Android 实机部署（音频出声 + AUDIO 锚点）待用户执行
