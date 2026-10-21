





//这是一个字段布局一个具体描述类型
//他是又filed type 展开信息并放入到FieldLayout，也就是说开发者不直接接触FieldLayout。
// //字段布局
#[derive(Clone, Debug)]
pub(crate) struct FieldLayout {
    offset: u64,
    size: u64,
    alignment: u64,
}

impl FieldLayout {

    pub fn new(
        offset: u64,
        size: u64,
        alignment: u64,
    )->Self{
        Self { offset, size, alignment }
    }

    #[inline]
    pub fn offset(&self)->u64{
        self.offset
    }
    #[inline]
    pub fn size(&self)->u64{
        self.size
    }
    #[inline]
    pub fn alignment(&self)->u64{
        self.alignment
    }
    
}

