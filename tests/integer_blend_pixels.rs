#[test]
fn corrected_integer_targets_render_the_expected_pixels() {
    use windows::core::s;
    let host = host().expect("native WARP device required");
    let (root, root_blob) = root_signature(&host.device, &[], &[], false).unwrap();
    let vs = crate::host::compile(VS, s!("main"), s!("vs_5_0")).unwrap();
    let outputs = "struct Out {float4 a:SV_Target0; float4 b:SV_Target1; uint c:SV_Target2;};
        Out main(){Out o; o.a=o.b=float4(1,0,0,0.5); o.c=7u; return o;}";
    let ps = crate::host::compile(outputs, s!("main"), s!("ps_5_0")).unwrap();
    let mesh_supported = host.mesh_shaders();
    eprintln!("WARP mesh shaders = {mesh_supported}, depth bounds = {}", host.depth_bounds());
    let mesh_code = if mesh_supported {
        let dir = tempfile::tempdir().unwrap();
        let dxc = dxc().expect("PC DXC needed to verify the supported mesh path");
        Some((compile(&dxc, dir.path(), MS, "ms_6_5"), compile(&dxc, dir.path(), outputs, "ps_6_5")))
    } else { None };
    for independent in [false, true] {
        let mut g = mixed_blend_state();
        g.depth.DepthEnable = false.into();
        g.rasterizer.CullMode = D3D12_CULL_MODE_NONE;
        if independent {
            g.blend.IndependentBlendEnable = true.into();
            g.blend.RenderTarget = [g.blend.RenderTarget[0]; 8];
            g.blend.RenderTarget[1].RenderTargetWriteMask = 2;
        }
        let blend = g.host_blend();
        if let Some((ms, mesh_ps)) = &mesh_code {
            let state = g.describe_with_blend(&blend);
            let name = super::super::library::name("integer MRT mesh test", &root_blob, &[ms, mesh_ps], &state);
            mesh_pipeline(&host.device, &root, [code(ms), code(mesh_ps)], &g, blend, false, &name)
                .expect("mesh pipeline with writable R8_UINT");
            eprintln!("mixed-target mesh PSO created for independent = {independent}");
        }
        let desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            VS: code(&vs), PS: code(&ps), BlendState: blend,
            SampleMask: g.sample_mask, RasterizerState: g.rasterizer, DepthStencilState: g.depth,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 3, RTVFormats: g.targets.RTFormats, SampleDesc: g.samples,
            ..Default::default()
        };
        let pso: ID3D12PipelineState = unsafe { host.device.CreateGraphicsPipelineState(&desc) }.unwrap();
        let get_resource = |heap_type, rd, state| -> ID3D12Resource {
            let heap = D3D12_HEAP_PROPERTIES { Type: heap_type, ..Default::default() };
            let mut out = None;
            unsafe { host.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &rd, state, None, &mut out) }.unwrap();
            out.unwrap()
        };
        let textures: Vec<_> = g.targets.RTFormats[..3].iter().map(|&format| get_resource(
            D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D, Width: 4, Height: 4, DepthOrArraySize: 1,
                MipLevels: 1, Format: format, SampleDesc: g.samples, Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                ..Default::default()
            }, D3D12_RESOURCE_STATE_RENDER_TARGET)).collect();
        let readback = get_resource(D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: 3072, Height: 1, DepthOrArraySize: 1,
            MipLevels: 1, SampleDesc: g.samples, Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR, ..Default::default()
        }, D3D12_RESOURCE_STATE_COPY_DEST);
        let views: Vec<_> = textures.iter().map(|t|host.rtv(Some(t), None)).collect();
        let list = host.new_list().unwrap();
        unsafe {
            for (&view, color) in views.iter().zip([[0.,0.,1.,1.], [0.,1.,0.,1.], [0.;4]]) {
                list.list.ClearRenderTargetView(view, &color, None);
            }
            list.list.OMSetRenderTargets(3, Some(views.as_ptr()), false, None);
            list.list.RSSetViewports(&[D3D12_VIEWPORT { Width: 4., Height: 4., MaxDepth: 1., ..Default::default() }]);
            list.list.RSSetScissorRects(&[windows::Win32::Foundation::RECT { left:0, top:0, right:4, bottom:4 }]);
            list.list.SetGraphicsRootSignature(&root);
            list.list.SetPipelineState(&pso);
            list.list.IASetPrimitiveTopology(windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.list.DrawInstanced(3, 1, 0, 0);
            for (slot, texture) in textures.iter().enumerate() {
                list.list.ResourceBarrier(&[crate::host::transition(texture, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
                let src = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(texture), Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 }
                };
                let dst = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&readback), Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 1024 * slot as u64, Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: g.targets.RTFormats[slot], Width: 4, Height: 4, Depth: 1, RowPitch: 256
                        } } }
                };
                list.list.CopyTextureRegion(&dst,0,0,0,&src,None);
            }
        }
        list.close();
        host.execute(&[&list]);
        let mut ptr = std::ptr::null_mut();
        unsafe { readback.Map(0,None,Some(&mut ptr)) }.unwrap();
        let pixels = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(),3072) };
        eprintln!("independent = {independent}: RT0 {:?}; RT1 {:?}; R8_UINT RT2 {}", &pixels[..4], &pixels[1024..1028], pixels[2048]);
        for y in 0..4 {
            for x in 0..4 {
                let i=y*256+x*4;
                for (got, want) in pixels[i..i+4].iter().zip([128,0,127,255]) {
                    assert!(got.abs_diff(want)<=1);
                }
                let want = if independent { [0,127,0,255] } else { [128,127,0,255] };
                for (got, want) in pixels[1024+i..1028+i].iter().zip(want) {
                    assert!(got.abs_diff(want)<=1);
                }
                assert_eq!(pixels[2048+y*256+x],7);
            }
        }
        unsafe { readback.Unmap(0,None); }
    }
}
