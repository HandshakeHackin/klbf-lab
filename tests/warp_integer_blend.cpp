#include <windows.h>
#include <d3d12.h>
#include <d3dcompiler.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstdio>
#include <climits>
#include <cstring>
#include <stdexcept>

using Microsoft::WRL::ComPtr;

void check(HRESULT hr, const char* operation) {
    if (FAILED(hr)) {
        std::fprintf(stderr, "%s failed (0x%08lX)\n", operation, static_cast<unsigned long>(hr));
        throw std::runtime_error(operation);
    }
}

ComPtr<ID3DBlob> compile(const char* source, const char* profile) {
    ComPtr<ID3DBlob> binary, errors;
    const auto hr = D3DCompile(source, std::strlen(source), nullptr, nullptr, nullptr,
                               "main", profile, 0, 0, &binary, &errors);
    if (errors) std::fwrite(errors->GetBufferPointer(), 1, errors->GetBufferSize(), stderr);
    check(hr, "D3DCompile");
    return binary;
}

int main() {
    ComPtr<IDXGIFactory4> factory;
    check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "create DXGI factory");
    ComPtr<IDXGIAdapter> warp;
    check(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)), "find WARP");
    DXGI_ADAPTER_DESC adapter{};
    check(warp->GetDesc(&adapter), "describe WARP");
    std::printf("WARP adapter: vendor=0x%04X device=0x%04X\n", adapter.VendorId, adapter.DeviceId);
    ComPtr<ID3D12Device> device;
    check(D3D12CreateDevice(warp.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)), "create WARP device");
    D3D12_FEATURE_DATA_FORMAT_SUPPORT support{DXGI_FORMAT_R8_UINT};
    check(device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT, &support, sizeof(support)), "query R8_UINT");
    std::printf("R8_UINT: render target=%d, blendable=%d\n",
        !!(support.Support1 & D3D12_FORMAT_SUPPORT1_RENDER_TARGET), !!(support.Support1 & D3D12_FORMAT_SUPPORT1_BLENDABLE));

    D3D12_ROOT_SIGNATURE_DESC rd{};
    ComPtr<ID3DBlob> blob;
    check(D3D12SerializeRootSignature(&rd, D3D_ROOT_SIGNATURE_VERSION_1, &blob, nullptr), "serialize root signature");
    ComPtr<ID3D12RootSignature> root;
    check(device->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(), IID_PPV_ARGS(&root)), "create root signature");
    auto vs = compile("float4 main(uint id:SV_VertexID):SV_Position { return float4(float(id),0,0,1); }", "vs_5_0");
    auto ps = compile("struct Out {float4 a:SV_Target0; float4 b:SV_Target1; uint c:SV_Target2;};"
                      "Out main(){ Out o; o.a=o.b=float4(1,0,0,0.5); o.c=7u; return o; }", "ps_5_0");
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pso{};
    pso.pRootSignature = root.Get();
    pso.VS = {vs->GetBufferPointer(), vs->GetBufferSize()};
    pso.PS = {ps->GetBufferPointer(), ps->GetBufferSize()};
    pso.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pso.RasterizerState.DepthClipEnable = TRUE;
    pso.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_LESS;
    pso.SampleMask = UINT_MAX;
    pso.NumRenderTargets = 3;
    pso.RTVFormats[0] = pso.RTVFormats[1] = DXGI_FORMAT_R8G8B8A8_UNORM;
    pso.RTVFormats[2] = DXGI_FORMAT_R8_UINT;
    pso.SampleDesc.Count = 1;
    D3D12_RENDER_TARGET_BLEND_DESC rt{};
    rt.SrcBlend = D3D12_BLEND_SRC_ALPHA;
    rt.DestBlend = D3D12_BLEND_INV_SRC_ALPHA;
    rt.BlendOp = rt.BlendOpAlpha = D3D12_BLEND_OP_ADD;
    rt.SrcBlendAlpha = D3D12_BLEND_ONE;
    rt.DestBlendAlpha = D3D12_BLEND_ZERO;
    rt.LogicOp = D3D12_LOGIC_OP_NOOP;
    rt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    struct Case { const char* name; bool independent, blend0, blend2, write2, control; };
    const Case cases[] = {
        {"shared, blending disabled", false,false,false,true,true},
        {"shared, RT0 blend inherited at RT2", false,true,false,true,false},
        {"independent, blend on normalized targets only", true,true,false,true,true},
        {"independent, explicit RT2 blend", true,true,true,true,false},
        {"independent, explicit RT2 blend with no writes", true,true,true,false,false},
        {"independent, no RT2 blend or writes", true,true,false,false,true},
    };
    bool controls_pass = true;
    for (const auto& c : cases) {
        pso.BlendState.IndependentBlendEnable = c.independent;
        for (auto& slot : pso.BlendState.RenderTarget) slot = rt;
        pso.BlendState.RenderTarget[0].BlendEnable = pso.BlendState.RenderTarget[1].BlendEnable = c.blend0;
        pso.BlendState.RenderTarget[2].BlendEnable = c.blend2;
        pso.BlendState.RenderTarget[2].RenderTargetWriteMask = c.write2 ? D3D12_COLOR_WRITE_ENABLE_ALL : 0;
        ComPtr<ID3D12PipelineState> result;
        const auto hr = device->CreateGraphicsPipelineState(&pso, IID_PPV_ARGS(&result));
        std::printf("%s -> 0x%08lX\n", c.name, static_cast<unsigned long>(hr));
        if (c.control && FAILED(hr)) controls_pass = false;
    }
    return controls_pass ? 0 : 1;
}
