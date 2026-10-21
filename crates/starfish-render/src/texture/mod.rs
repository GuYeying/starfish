

mod texture_desc;
mod texture;


pub use texture_desc::{TextureDescriptor,MipmapPolicy,TextureDim,TextureSemantic,TextureUsage};
pub use texture::{Texture,ViewKey};