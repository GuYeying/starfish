# 资源包与 io 分层设计（2026-11-01 定约）

## 一、目标

同一份 Python 调用（`pygame.io.read/read_text/exists/write_text`）跨构建不变：
- **dev 构建**：纯路径直读（松散文件）
- **runtime 构建**：资源包读取（+ 可选松散覆盖目录——热补丁/DLC/mod）
- **wasm**：多文件 fetch（用户定约）——逐文件加密散文件

## 二、分层

```
pygame.io.read(path)
  └─ starfish-io 解析链（init 时定约，创建时一次性）：
     [dev]     base_dir/path（std::fs / web fetch）
     [runtime] 松散覆盖目录(可选) → 资源包按序查找 → 错误
  └─ 解密层（与打包解耦）：封装探测（magic）→ 有封装 = AEAD 解密
     （密钥 = init 注入；AAD = 路径，防挪用）；无封装 = 原样透传
```

- 写入恒走 base_dir（包只读）——补丁/DLC/mod 走松散文件
- 未 init = 现行为（纯路径），向后兼容

## 三、包格式 SFIX1（自研极简，索引尾置）

```
偏移 0   : magic "SFIX1"（6B）
偏移 6   : version u32 LE
偏移 10  : flags u32 LE（bit0 = 索引已加密）
偏移 14  : index_nonce 12B（索引 ChaCha20-Poly1305 nonce，明文存放）
偏移 26  : index_offset u64 LE
偏移 34  : index_len  u64 LE
偏移 42  : [数据区：逐条目封装]
索引区   : XChaCha20-Poly1305 密文（AAD = b"sf-index"）——明文 = JSON：
           { "entries": [ {"p": 路径, "o": 封装偏移, "l": 封装长度} ] }
```

- **条目封装**（包内与 wasm 散文件同格式）：`"SFXE1" magic(6) | nonce(12) | 密文+tag(16)`
  - 算法 = IETF ChaCha20-Poly1305（12B nonce——与 Python cryptography 原生互操作）
  - AAD = 条目路径（包内）/ fetch 路径（wasm 散文件）——身份绑定防挪用
  - 整条目一次 AEAD（io.read 本就是整文件读——随机访问需求 = 包级 offset 定位）
- 索引尾置 = 打包器流式写；Poly1305 tag = 完整性内建（无需额外哈希）

## 四、加密定约

- 算法：IETF ChaCha20-Poly1305（RustCrypto `chacha20poly1305` crate——纯 Rust、
  no_std、wasm/移动/桌面性能一致；Poly1305 认证 = 保密+防篡改一体）
- 密钥：32B，宿主创建时注入（`IoConfig.key`）；**同一密钥覆盖包与 wasm
  散文件**（打包与加密解耦的兑现）
- 威胁模型诚实说明：防资源提取器/随手解包（格式自研 + AEAD + 索引加密 =
  无现成工具可开）；密钥在客户端 = 不防逆向工程师（行业现实）
- 二次压缩：**不做**（PNG/音频/字体已压缩，收益 0-5% 白付 CPU；格式预留
  flags 位，v2 按需）

## 五、wasm 定约（用户拍板）

- **仅多文件 fetch**（并发拉取解决加载慢）；服务器上 = 加密散文件
- 同一份密钥 init 注入 → fetch 后解密——与包路径共用封装格式
- 整包 fetch（内存包）未来可加（设计不排斥，v1 不做）

## 六、实施

1. starfish-io：IoConfig/init（创建时）+ 解析链 + SFIX1 读 + AEAD 解密
   （封装自动探测，明文资源透传——混合加密/明文资源无缝）
2. 打包器：pygame-rs-dev/scripts/pack_assets.py（Python cryptography 库
   加密——与 Rust 侧 XChaCha20-Poly1305 互操作）
3. bundle 脚本 --pack 选项：resources/ → game.pak
4. Python 面零改动
