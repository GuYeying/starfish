//! starfish-ds：数据结构层（**stdlite 定约**——Rust 直算零 py 编组）。
//!
//! 设计定约（2026-11-24 用户裁决）：**算法全在 `impl` 方法**（py 调用
//! = 触发器/批量口，值载荷泛型 f64 优先级 + py 对象值——A* 主场景 =
//! f32 cost 直算不回 py `__lt__`）。
//!
//! py 面 = pygame.ds（pygamest 绑定）。
//!
//! 成员：
//! - [`Heap`]：二叉堆（f64 优先级最小堆——A* open list 主力）
//! - [`SortedList`]：有序数组（二分插入/索引——bisect 等价）
//! - [`UnionFind`]：路径压缩 + 按秩合并（连通性/迷宫生成）

/// 二叉最小堆（f64 优先级 + usize 载荷索引——A* 节点表配对形态；
/// py 值载荷由绑定层 Vec 侧表持有，本结构只承载索引 = 零对象编组）
#[derive(Debug, Clone, Default)]
pub struct Heap {
    /// (priority, payload_index) 平铺——最小堆（priority 小者先出）
    items: Vec<(f64, usize)>,
}

impl Heap {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 入堆（sift-up——标准二叉堆上浮）
    pub fn push(&mut self, priority: f64, payload_index: usize) {
        self.items.push((priority, payload_index));
        let mut i = self.items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if self.items[parent].0 <= self.items[i].0 {
                break;
            }
            self.items.swap(parent, i);
            i = parent;
        }
    }

    /// 出堆（最小 priority 项——sift-down 下沉；空堆 = None）
    pub fn pop(&mut self) -> Option<(f64, usize)> {
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        self.items.swap(0, last);
        let out = self.items.pop();
        let mut i = 0;
        let n = self.items.len();
        loop {
            let l = i * 2 + 1;
            let r = i * 2 + 2;
            let mut best = i;
            if l < n && self.items[l].0 < self.items[best].0 {
                best = l;
            }
            if r < n && self.items[r].0 < self.items[best].0 {
                best = r;
            }
            if best == i {
                break;
            }
            self.items.swap(i, best);
            i = best;
        }
        out
    }

    /// 入堆并立即出堆（A* 高频组合——比 push+pop 少一次上浮）
    pub fn push_pop(&mut self, priority: f64, payload_index: usize) -> (f64, usize) {
        if self.items.is_empty() || self.items[0].0 <= priority {
            return (priority, payload_index);
        }
        let out = self.items[0];
        self.items[0] = (priority, payload_index);
        let n = self.items.len();
        let mut i = 0;
        loop {
            let l = i * 2 + 1;
            let r = i * 2 + 2;
            let mut best = i;
            if l < n && self.items[l].0 < self.items[best].0 {
                best = l;
            }
            if r < n && self.items[r].0 < self.items[best].0 {
                best = r;
            }
            if best == i {
                break;
            }
            self.items.swap(i, best);
            i = best;
        }
        out
    }

    /// 批量建堆（Floyd 线性下滤——从列表一次性构造）
    pub fn from_pairs(pairs: Vec<(f64, usize)>) -> Self {
        let mut heap = Self { items: pairs };
        let n = heap.items.len();
        for i in (0..n / 2).rev() {
            heap.sift_down_from(i);
        }
        heap
    }

    fn sift_down_from(&mut self, start: usize) {
        let n = self.items.len();
        let mut i = start;
        loop {
            let l = i * 2 + 1;
            let r = i * 2 + 2;
            let mut best = i;
            if l < n && self.items[l].0 < self.items[best].0 {
                best = l;
            }
            if r < n && self.items[r].0 < self.items[best].0 {
                best = r;
            }
            if best == i {
                break;
            }
            self.items.swap(i, best);
            i = best;
        }
    }
}

/// 有序数组（二分插入/索引——bisect 等价；f64 键排序 + usize 载荷
/// 索引配对，A* 关闭表/调度表形态）
#[derive(Debug, Clone, Default)]
pub struct SortedList {
    items: Vec<(f64, usize)>,
}

impl SortedList {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 二分查找插入位（bisect_left 语义——相同键取左侧）
    pub fn bisect_left(&self, key: f64) -> usize {
        let mut lo = 0usize;
        let mut hi = self.items.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.items[mid].0 < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// 二分查找插入位（bisect_right 语义——相同键取右侧）
    pub fn bisect_right(&self, key: f64) -> usize {
        let mut lo = 0usize;
        let mut hi = self.items.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.items[mid].0 <= key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// 有序插入（自动定位——O(n) 移位不可避免，O(log) 定位）
    pub fn insert(&mut self, key: f64, payload_index: usize) {
        let pos = self.bisect_left(key);
        self.items.insert(pos, (key, payload_index));
    }

    /// 按索引取（有序第 i 项）
    pub fn get(&self, index: usize) -> Option<(f64, usize)> {
        self.items.get(index).copied()
    }

    /// 键存在性（二分——O(log)）
    pub fn contains(&self, key: f64) -> bool {
        let pos = self.bisect_left(key);
        pos < self.items.len() && self.items[pos].0 == key
    }

    /// 移除指定索引项
    pub fn remove_at(&mut self, index: usize) -> Option<(f64, usize)> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    /// 首项（最小键）
    pub fn first(&self) -> Option<(f64, usize)> {
        self.items.first().copied()
    }

    /// 末项（最大键）
    pub fn last(&self) -> Option<(f64, usize)> {
        self.items.last().copied()
    }

    /// 范围切片（键 ∈ [lo, hi)——索引列表回传）
    pub fn range(&self, lo: f64, hi: f64) -> Vec<usize> {
        let start = self.bisect_left(lo);
        let end = self.bisect_right(hi);
        (start..end.min(self.items.len()))
            .map(|i| self.items[i].1)
            .collect()
    }
}

/// 并查集（路径压缩 + 按秩合并——连通性判定/迷宫生成/区域合并）
#[derive(Debug, Clone, Default)]
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
    components: usize,
}

impl UnionFind {
    /// n 个独立元素初始化
    pub fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
            rank: vec![0; n],
            components: n,
        }
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn components(&self) -> usize {
        self.components
    }

    /// 找根（路径压缩——两跳扁平化）
    pub fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // 路径压缩
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    /// 合并（按秩——不同树才合并，返回是否发生合并）
    pub fn union(&mut self, a: usize, b: usize) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
        self.components -= 1;
        true
    }

    /// 连通判定
    pub fn connected(&mut self, a: usize, b: usize) -> bool {
        self.find(a) == self.find(b)
    }
}
