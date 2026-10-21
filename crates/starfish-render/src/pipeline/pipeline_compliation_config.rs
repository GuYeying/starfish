use std::collections::HashMap;



#[derive(Clone, Debug)]
pub struct PipelineCompilationConfig {

    /// shader override constants
    pub constants: HashMap<String, f64>,


    /// 是否初始化 workgroup memory
    pub zero_initialize_workgroup_memory: bool,
}


impl PipelineCompilationConfig{

    pub fn set_constant(
        &mut self,
        name: impl Into<String>,
        value:f64,
    ){

        self.constants.insert(
            name.into(),
            value,
        );

    }


    pub fn constant(
        mut self,
        name:impl Into<String>,
        value:f64,
    )->Self {

        self.constants.insert(
            name.into(),
            value,
        );

        self

    }
    
}
impl Default for PipelineCompilationConfig {

    fn default() -> Self {

        Self {

            constants: HashMap::new(),

            zero_initialize_workgroup_memory:true,

        }

    }
}