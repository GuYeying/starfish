| 修改内容                             | 需要重建 BindGroup | 需要重建 Pipeline |
| -------------------------------- | -------------- | ------------- |
| Uniform 数据（`queue.write_buffer`） | ❌              | ❌             |
| Uniform Buffer 换成新的 Buffer       | ✅              | ❌             |
| Texture 换成另一张 Texture            | ✅              | ❌             |
| TextureView 改变                   | ✅              | ❌             |
| Sampler 改变                       | ✅              | ❌             |
| BindGroupLayout 改变               | ✅              | ✅             |
| Shader 改变                        | ——             | ✅             |
| VertexLayout 改变                  | ——             | ✅             |
| Blend/Cull/Depth 等状态改变           | ——             | ✅             |
