//! Root signatures for the shaders we compile ourselves: read from the
//! `RS` string in the HLSL (`#define RS "..."`) and serialized from a
//! description built from it, not taken out of the compiled blob. Under
//! Wine the host refuses the embedded signature of the blob its compiler
//! returns (E_FAIL), while a description we serialize ourselves works, as
//! the title's own signatures do. Reading the string from the same source
//! the shaders were compiled from keeps the description and what the
//! shaders expect one and the same, word for word.

use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Error;

/// The root signature of the shaders compiled from `source`, read from its
/// `#define RS "..."`.
pub(crate) fn make(device: &ID3D12Device, source: &str) -> windows::core::Result<ID3D12RootSignature> {
    const MARK: &str = "#define RS \"";
    let refused = |why: &str| {
        tracing::warn!(why, "a root signature of ours could not be read from the shader's source");
        Error::empty()
    };
    let Some(at) = source.find(MARK) else {
        return Err(refused("the source carries no RS"));
    };
    let rest = &source[at + MARK.len()..];
    let Some(end) = rest.find('"') else {
        return Err(refused("the RS is never closed"));
    };
    let sig = parse(&rest[..end]).ok_or_else(|| refused("the RS is of a word not understood"))?;
    serialize(device, &sig)
}

/// What a parsed RS string is of, before it points into arrays.
struct Sig {
    params: Vec<Param>,
    samplers: Vec<D3D12_STATIC_SAMPLER_DESC>,
    flags: D3D12_ROOT_SIGNATURE_FLAGS,
}

enum Param {
    Constants(D3D12_ROOT_CONSTANTS, D3D12_SHADER_VISIBILITY),
    Descriptor(D3D12_ROOT_PARAMETER_TYPE, D3D12_ROOT_DESCRIPTOR, D3D12_SHADER_VISIBILITY),
    Table(Vec<D3D12_DESCRIPTOR_RANGE>, D3D12_SHADER_VISIBILITY),
}

fn parse(text: &str) -> Option<Sig> {
    let mut sig = Sig {
        params: Vec::new(),
        samplers: Vec::new(),
        flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    for piece in split_top(text) {
        let (name, args) = element(piece.trim())?;
        match name {
            "RootFlags" => sig.flags = D3D12_ROOT_SIGNATURE_FLAGS(flags(args)),
            "RootConstants" => sig.params.push(constants(args)?),
            "SRV" => sig.params.push(descriptor(D3D12_ROOT_PARAMETER_TYPE_SRV, args)?),
            "UAV" => sig.params.push(descriptor(D3D12_ROOT_PARAMETER_TYPE_UAV, args)?),
            "CBV" => sig.params.push(descriptor(D3D12_ROOT_PARAMETER_TYPE_CBV, args)?),
            "DescriptorTable" => sig.params.push(table(args)?),
            "StaticSampler" => sig.samplers.push(sampler(args)?),
            _ => return None,
        }
    }
    Some(sig)
}

/// The pieces `text` is of: its top-level words, commas between
/// parentheses passed over.
fn split_top(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    pieces.push(&text[start..]);
    pieces
}

/// An element as its name and what is between its parentheses.
fn element(piece: &str) -> Option<(&str, &str)> {
    let (name, rest) = piece.split_once('(')?;
    let end = rest.rfind(')')?;
    Some((name.trim(), &rest[..end]))
}

/// A `key = value` word.
fn pair(word: &str) -> Option<(&str, &str)> {
    word.split_once('=').map(|(key, value)| (key.trim(), value.trim()))
}

/// A register word like `t3`: its number, of the kind `letter` names.
fn register(word: &str, letter: char) -> Option<u32> {
    let (first, number) = word.split_at(1);
    (first.chars().next()? == letter).then(|| number.parse().ok())?
}

fn constants(args: &str) -> Option<Param> {
    let mut count = None;
    let mut register_at = None;
    let mut space = 0;
    let mut visibility = D3D12_SHADER_VISIBILITY_ALL;
    for word in split_top(args) {
        let word = word.trim();
        match pair(word) {
            Some(("num32BitConstants", value)) => count = value.parse().ok(),
            Some(("visibility", value)) => visibility = shader_visibility(value)?,
            Some(("space", value)) => space = value.parse().ok()?,
            Some(_) => return None,
            None => register_at = register(word, 'b'),
        }
    }
    Some(Param::Constants(
        D3D12_ROOT_CONSTANTS {
            Num32BitValues: count?,
            ShaderRegister: register_at?,
            RegisterSpace: space,
        },
        visibility,
    ))
}

fn descriptor(kind: D3D12_ROOT_PARAMETER_TYPE, args: &str) -> Option<Param> {
    let letter = match kind {
        D3D12_ROOT_PARAMETER_TYPE_SRV => 't',
        D3D12_ROOT_PARAMETER_TYPE_UAV => 'u',
        _ => 'b',
    };
    let mut register_at = None;
    let mut space = 0;
    let mut visibility = D3D12_SHADER_VISIBILITY_ALL;
    for word in split_top(args) {
        let word = word.trim();
        match pair(word) {
            Some(("visibility", value)) => visibility = shader_visibility(value)?,
            Some(("space", value)) => space = value.parse().ok()?,
            Some(_) => return None,
            None => register_at = register(word, letter),
        }
    }
    Some(Param::Descriptor(
        kind,
        D3D12_ROOT_DESCRIPTOR { ShaderRegister: register_at?, RegisterSpace: space },
        visibility,
    ))
}

fn table(args: &str) -> Option<Param> {
    let mut visibility = D3D12_SHADER_VISIBILITY_ALL;
    let mut ranges = Vec::new();
    for word in split_top(args) {
        let word = word.trim();
        if word.contains('(') {
            ranges.push(range(word)?);
        } else {
            match pair(word) {
                Some(("visibility", value)) => visibility = shader_visibility(value)?,
                _ => return None,
            }
        }
    }
    Some(Param::Table(ranges, visibility))
}

/// One range of a table: `SRV(t0, numDescriptors=2)` and such.
fn range(word: &str) -> Option<D3D12_DESCRIPTOR_RANGE> {
    let (name, args) = element(word.trim())?;
    let kind = match name {
        "SRV" => D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        "UAV" => D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
        "CBV" => D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
        "Sampler" => D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
        _ => return None,
    };
    let letter = match kind {
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV => 't',
        D3D12_DESCRIPTOR_RANGE_TYPE_UAV => 'u',
        D3D12_DESCRIPTOR_RANGE_TYPE_CBV => 'b',
        _ => 's',
    };
    let mut base = None;
    let mut count = 1;
    let mut space = 0;
    for piece in split_top(args) {
        let word = piece.trim();
        match pair(word) {
            // The 1.0 serialization carries no range flags; the strings that
            // name them are the title's volatile tables as ours already are.
            Some(("flags", _)) => {}
            Some(("numDescriptors", value)) => count = value.parse().ok()?,
            Some(("space", value)) => space = value.parse().ok()?,
            Some(_) => return None,
            None => base = register(word, letter),
        }
    }
    Some(D3D12_DESCRIPTOR_RANGE {
        RangeType: kind,
        NumDescriptors: count,
        BaseShaderRegister: base?,
        RegisterSpace: space,
        OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
    })
}

fn sampler(args: &str) -> Option<D3D12_STATIC_SAMPLER_DESC> {
    // The grammar's defaults, for the words the string leaves out.
    let mut desc = D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_ANISOTROPIC,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
        MipLODBias: 0.0,
        MaxAnisotropy: 16,
        ComparisonFunc: D3D12_COMPARISON_FUNC_NONE,
        BorderColor: D3D12_STATIC_BORDER_COLOR_OPAQUE_BLACK,
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
        ShaderRegister: 0,
        RegisterSpace: 0,
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    };
    let mut register_at = None;
    for word in split_top(args) {
        let word = word.trim();
        match pair(word) {
            Some(("filter", value)) => desc.Filter = filter(value)?,
            Some(("addressU", value)) => desc.AddressU = address(value)?,
            Some(("addressV", value)) => desc.AddressV = address(value)?,
            Some(("addressW", value)) => desc.AddressW = address(value)?,
            Some(("mipLODBias", value)) => desc.MipLODBias = value.parse().ok()?,
            Some(("maxAnisotropy", value)) => desc.MaxAnisotropy = value.parse().ok()?,
            Some(("comparisonFunc", value)) => desc.ComparisonFunc = comparison(value)?,
            Some(("borderColor", value)) => desc.BorderColor = border(value)?,
            Some(("minLOD", value)) => desc.MinLOD = value.parse().ok()?,
            Some(("maxLOD", value)) => desc.MaxLOD = value.parse().ok()?,
            Some(("space", value)) => desc.RegisterSpace = value.parse().ok()?,
            Some(("visibility", value)) => desc.ShaderVisibility = shader_visibility(value)?,
            Some(_) => return None,
            None => register_at = register(word, 's'),
        }
    }
    desc.ShaderRegister = register_at?;
    Some(desc)
}

fn filter(name: &str) -> Option<D3D12_FILTER> {
    Some(match name {
        "FILTER_MIN_MAG_MIP_POINT" => D3D12_FILTER_MIN_MAG_MIP_POINT,
        "FILTER_MIN_MAG_POINT_MIP_LINEAR" => D3D12_FILTER_MIN_MAG_POINT_MIP_LINEAR,
        "FILTER_MIN_POINT_MAG_MIP_LINEAR" => D3D12_FILTER_MIN_POINT_MAG_MIP_LINEAR,
        "FILTER_MIN_LINEAR_MAG_MIP_POINT" => D3D12_FILTER_MIN_LINEAR_MAG_MIP_POINT,
        "FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR" => D3D12_FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR,
        "FILTER_MIN_MAG_MIP_LINEAR" => D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        "FILTER_MIN_MAG_LINEAR_MIP_POINT" => D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
        "FILTER_ANISOTROPIC" => D3D12_FILTER_ANISOTROPIC,
        "FILTER_COMPARISON_MIN_MAG_MIP_POINT" => D3D12_FILTER_COMPARISON_MIN_MAG_MIP_POINT,
        "FILTER_COMPARISON_MIN_MAG_MIP_LINEAR" => D3D12_FILTER_COMPARISON_MIN_MAG_MIP_LINEAR,
        "FILTER_COMPARISON_ANISOTROPIC" => D3D12_FILTER_COMPARISON_ANISOTROPIC,
        "FILTER_MINIMUM_MIN_MAG_MIP_LINEAR" => D3D12_FILTER_MINIMUM_MIN_MAG_MIP_LINEAR,
        "FILTER_MAXIMUM_MIN_MAG_MIP_LINEAR" => D3D12_FILTER_MAXIMUM_MIN_MAG_MIP_LINEAR,
        _ => return None,
    })
}

fn address(name: &str) -> Option<D3D12_TEXTURE_ADDRESS_MODE> {
    Some(match name {
        "TEXTURE_ADDRESS_WRAP" => D3D12_TEXTURE_ADDRESS_MODE_WRAP,
        "TEXTURE_ADDRESS_MIRROR" => D3D12_TEXTURE_ADDRESS_MODE_MIRROR,
        "TEXTURE_ADDRESS_CLAMP" => D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        "TEXTURE_ADDRESS_BORDER" => D3D12_TEXTURE_ADDRESS_MODE_BORDER,
        "TEXTURE_ADDRESS_MIRROR_ONCE" => D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE,
        _ => return None,
    })
}

fn comparison(name: &str) -> Option<D3D12_COMPARISON_FUNC> {
    Some(match name {
        "COMPARISON_NONE" => D3D12_COMPARISON_FUNC_NONE,
        "COMPARISON_LESS" => D3D12_COMPARISON_FUNC_LESS,
        "COMPARISON_EQUAL" => D3D12_COMPARISON_FUNC_EQUAL,
        "COMPARISON_LESS_EQUAL" => D3D12_COMPARISON_FUNC_LESS_EQUAL,
        "COMPARISON_GREATER" => D3D12_COMPARISON_FUNC_GREATER,
        "COMPARISON_NOT_EQUAL" => D3D12_COMPARISON_FUNC_NOT_EQUAL,
        "COMPARISON_GREATER_EQUAL" => D3D12_COMPARISON_FUNC_GREATER_EQUAL,
        "COMPARISON_ALWAYS" => D3D12_COMPARISON_FUNC_ALWAYS,
        _ => return None,
    })
}

fn border(name: &str) -> Option<D3D12_STATIC_BORDER_COLOR> {
    Some(match name {
        "STATIC_BORDER_COLOR_OPAQUE_BLACK" => D3D12_STATIC_BORDER_COLOR_OPAQUE_BLACK,
        "STATIC_BORDER_COLOR_OPAQUE_WHITE" => D3D12_STATIC_BORDER_COLOR_OPAQUE_WHITE,
        "STATIC_BORDER_COLOR_TRANSPARENT_BLACK" => D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
        _ => return None,
    })
}

fn shader_visibility(name: &str) -> Option<D3D12_SHADER_VISIBILITY> {
    Some(match name {
        "SHADER_VISIBILITY_ALL" => D3D12_SHADER_VISIBILITY_ALL,
        "SHADER_VISIBILITY_VERTEX" => D3D12_SHADER_VISIBILITY_VERTEX,
        "SHADER_VISIBILITY_HULL" => D3D12_SHADER_VISIBILITY_HULL,
        "SHADER_VISIBILITY_DOMAIN" => D3D12_SHADER_VISIBILITY_DOMAIN,
        "SHADER_VISIBILITY_GEOMETRY" => D3D12_SHADER_VISIBILITY_GEOMETRY,
        "SHADER_VISIBILITY_PIXEL" => D3D12_SHADER_VISIBILITY_PIXEL,
        _ => return None,
    })
}

/// A root flag word: the names the strings use, or a number.
fn flags(text: &str) -> i32 {
    let mut value = 0;
    for name in text.split('|') {
        let bit = match name.trim() {
            "0" => 0,
            "ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT" => D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT.0,
            "ALLOW_STREAM_OUTPUT" => D3D12_ROOT_SIGNATURE_FLAG_ALLOW_STREAM_OUTPUT.0,
            "DENY_VERTEX_SHADER_ROOT_ACCESS" => D3D12_ROOT_SIGNATURE_FLAG_DENY_VERTEX_SHADER_ROOT_ACCESS.0,
            "DENY_HULL_SHADER_ROOT_ACCESS" => D3D12_ROOT_SIGNATURE_FLAG_DENY_HULL_SHADER_ROOT_ACCESS.0,
            "DENY_DOMAIN_SHADER_ROOT_ACCESS" => D3D12_ROOT_SIGNATURE_FLAG_DENY_DOMAIN_SHADER_ROOT_ACCESS.0,
            "DENY_GEOMETRY_SHADER_ROOT_ACCESS" => D3D12_ROOT_SIGNATURE_FLAG_DENY_GEOMETRY_SHADER_ROOT_ACCESS.0,
            "DENY_PIXEL_SHADER_ROOT_ACCESS" => D3D12_ROOT_SIGNATURE_FLAG_DENY_PIXEL_SHADER_ROOT_ACCESS.0,
            _ => return 0,
        };
        value |= bit;
    }
    value
}

fn serialize(device: &ID3D12Device, sig: &Sig) -> windows::core::Result<ID3D12RootSignature> {
    // The tables' ranges are pointed at, so they are flattened into one
    // array that lives through the serialization, then the parameters.
    let mut ranges: Vec<D3D12_DESCRIPTOR_RANGE> = Vec::new();
    let mut tables: Vec<(u32, u32)> = Vec::new();
    for param in &sig.params {
        if let Param::Table(own, _) = param {
            tables.push((ranges.len() as u32, own.len() as u32));
            ranges.extend_from_slice(own);
        }
    }
    let mut left = tables.iter();
    let params: Vec<D3D12_ROOT_PARAMETER> = sig
        .params
        .iter()
        .map(|param| match param {
            Param::Table(_, visibility) => {
                let (first, count) = *left.next()?;
                Some(D3D12_ROOT_PARAMETER {
                    ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                    ShaderVisibility: *visibility,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                            NumDescriptorRanges: count,
                            pDescriptorRanges: ranges[first as usize..].as_ptr(),
                        },
                    },
                })
            }
            Param::Constants(constants, visibility) => Some(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                ShaderVisibility: *visibility,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: *constants },
            }),
            Param::Descriptor(kind, descriptor, visibility) => Some(D3D12_ROOT_PARAMETER {
                ParameterType: *kind,
                ShaderVisibility: *visibility,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: *descriptor },
            }),
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(Error::empty)?;
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: params.len() as u32,
        pParameters: params.as_ptr(),
        NumStaticSamplers: sig.samplers.len() as u32,
        pStaticSamplers: sig.samplers.as_ptr(),
        Flags: sig.flags,
    };
    // SAFETY: the description points at the live arrays; the blob is read
    // in full before it is released.
    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        let mut errors: Option<ID3DBlob> = None;
        if let Err(e) = D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1_0, &mut blob, Some(&mut errors)) {
            if let Some(errors) = errors {
                let text = std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                );
                tracing::warn!("root signature: {}", String::from_utf8_lossy(text));
            }
            return Err(e);
        }
        let blob = blob.ok_or_else(Error::empty)?;
        let bytes =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
        device.CreateRootSignature(0, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin_signatures() -> Vec<String> {
        const MARK: &str = "#define RS \"";
        let sources = [
            include_str!("ambient.rs"),
            include_str!("compose.rs"),
            include_str!("host.rs"),
            include_str!("indirect.rs"),
            include_str!("replay/play/merge.rs"),
        ];
        let mut signatures = Vec::new();
        for source in sources {
            let mut rest = source;
            while let Some(at) = rest.find(MARK) {
                let after = &rest[at + MARK.len()..];
                let end = after.find('"').expect("root signature string is closed");
                signatures.push(after[..end].to_owned());
                rest = &after[end + 1..];
            }
        }
        let lightprobe = include_str!("lightprobe.rs");
        for marker in ["const RS_EXPOSURE: &str = \"", "const RS_LUX: &str = \""] {
            let at = lightprobe.find(marker).expect("lightprobe signature exists");
            let after = &lightprobe[at + marker.len()..];
            let end = after.find('"').expect("lightprobe signature is closed");
            signatures.push(after[..end].to_owned());
        }
        assert_eq!(signatures.len(), 18, "built-in signature inventory changed");
        signatures
    }

    fn compiler_diagnostic(source: &str, entry: windows::core::PCSTR) -> String {
        use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
        let mut code = None;
        let mut errors = None;
        let result = unsafe {
            D3DCompile(
                source.as_ptr().cast(),
                source.len(),
                windows::core::s!("victory"),
                None,
                None,
                entry,
                windows::core::s!("cs_5_1"),
                0,
                0,
                &mut code,
                Some(&mut errors),
            )
        };
        match result {
            Ok(()) => "secondary D3DCompile call succeeded".to_owned(),
            Err(error) => errors.map_or_else(
                || format!("{error:?}; no compiler diagnostic blob"),
                |blob: ID3DBlob| unsafe {
                    let bytes = std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    );
                    String::from_utf8_lossy(bytes).into_owned()
                },
            ),
        }
    }

    #[test]
    fn recognizes_supported_filter_constants() {
        let cases = [
            ("FILTER_MIN_MAG_MIP_POINT", D3D12_FILTER_MIN_MAG_MIP_POINT),
            ("FILTER_MIN_MAG_POINT_MIP_LINEAR", D3D12_FILTER_MIN_MAG_POINT_MIP_LINEAR),
            ("FILTER_MIN_POINT_MAG_MIP_LINEAR", D3D12_FILTER_MIN_POINT_MAG_MIP_LINEAR),
            ("FILTER_MIN_LINEAR_MAG_MIP_POINT", D3D12_FILTER_MIN_LINEAR_MAG_MIP_POINT),
            ("FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR", D3D12_FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR),
            ("FILTER_MIN_MAG_MIP_LINEAR", D3D12_FILTER_MIN_MAG_MIP_LINEAR),
            ("FILTER_MIN_MAG_LINEAR_MIP_POINT", D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT),
            ("FILTER_ANISOTROPIC", D3D12_FILTER_ANISOTROPIC),
            ("FILTER_COMPARISON_MIN_MAG_MIP_POINT", D3D12_FILTER_COMPARISON_MIN_MAG_MIP_POINT),
            ("FILTER_COMPARISON_MIN_MAG_MIP_LINEAR", D3D12_FILTER_COMPARISON_MIN_MAG_MIP_LINEAR),
            ("FILTER_COMPARISON_ANISOTROPIC", D3D12_FILTER_COMPARISON_ANISOTROPIC),
            ("FILTER_MINIMUM_MIN_MAG_MIP_LINEAR", D3D12_FILTER_MINIMUM_MIN_MAG_MIP_LINEAR),
            ("FILTER_MAXIMUM_MIN_MAG_MIP_LINEAR", D3D12_FILTER_MAXIMUM_MIN_MAG_MIP_LINEAR),
        ];
        for (name, expected) in cases {
            assert_eq!(filter(name), Some(expected), "{name}");
        }
        assert_eq!(filter("FILTER_UNKNOWN"), None);
    }

    #[test]
    fn parses_builtin_shader_root_signatures() {
        for signature in builtin_signatures() {
            assert!(parse(&signature).is_some(), "unsupported root signature: {signature}");
        }
    }

    #[test]
    #[ignore = "requires a working D3D12 host device"]
    fn creates_builtin_root_signatures_on_host_device() {
        let mut device: Option<ID3D12Device> = None;
        unsafe {
            D3D12CreateDevice(
                None::<&windows::core::IUnknown>,
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0,
                &mut device,
            )
            .expect("D3D12 device creation");
        }
        let device = device.expect("D3D12CreateDevice returned no device");
        let luid = unsafe { device.GetAdapterLuid() };
        let factory = unsafe {
            windows::Win32::Graphics::Dxgi::CreateDXGIFactory1::<
                windows::Win32::Graphics::Dxgi::IDXGIFactory4,
            >()
            .expect("DXGI factory creation")
        };
        let adapter = unsafe {
            factory
                .EnumAdapterByLuid::<windows::Win32::Graphics::Dxgi::IDXGIAdapter1>(luid)
                .expect("adapter matching the D3D12 device")
        };
        let desc = unsafe { adapter.GetDesc1().expect("adapter description") };
        let name_end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let adapter_name = String::from_utf16_lossy(&desc.Description[..name_end]);
        let signatures = builtin_signatures();
        for signature in &signatures {
            let source = format!("#define RS \"{signature}\"");
            make(&device, &source).unwrap_or_else(|e| {
                panic!("root signature serialization or creation failed for {signature}: {e:?}")
            });
        }
        let control_source = r#"
#define RS "UAV(u0)"
RWByteAddressBuffer data : register(u0);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() { data.Store(0, 1); }
"#;
        let control_code = crate::host::compile(
            control_source,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
        )
        .expect("simple Store-only compute shader compilation");
        let control_root = make(&device, control_source).expect("simple compute root signature");
        let control_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&control_root) },
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: control_code.as_ptr().cast(),
                BytecodeLength: control_code.len(),
            },
            ..Default::default()
        };
        let _control_pso: ID3D12PipelineState = unsafe {
            device
                .CreateComputePipelineState(&control_desc)
                .expect("simple Store-only compute PSO creation")
        };
        let readonly_load_source = r#"
#define RS "SRV(t0), UAV(u0)"
ByteAddressBuffer input : register(t0);
RWByteAddressBuffer output : register(u0);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() { output.Store4(0, input.Load4(0)); }
"#;
        let readonly_load_code = crate::host::compile(
            readonly_load_source,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
        )
        .expect("read-only ByteAddressBuffer Load4 shader compilation");
        let readonly_load_root = make(&device, readonly_load_source)
            .expect("read-only ByteAddressBuffer Load4 root signature");
        let readonly_load_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&readonly_load_root) },
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: readonly_load_code.as_ptr().cast(),
                BytecodeLength: readonly_load_code.len(),
            },
            ..Default::default()
        };
        let _readonly_load_pso: ID3D12PipelineState = unsafe {
            device
                .CreateComputePipelineState(&readonly_load_desc)
                .expect("read-only ByteAddressBuffer Load4 PSO creation")
        };
        // Compile the fork's actual batching shader with the values selected
        // for an AMD adapter by indirect::wide() and indirect::rest().
        let batch = include_str!("indirect/batch.rs");
        let batch_marker = "pub(crate) const SOURCE: &str = r#\"";
        let batch_at = batch.find(batch_marker).expect("indirect batch HLSL exists");
        let batch_after = &batch[batch_at + batch_marker.len()..];
        let batch_end = batch_after.find("\"#;").expect("indirect batch HLSL is closed");
        let batch_source = format!(
            "#define WIDEST 1048576u\n#define REST 1\n{}",
            &batch_after[..batch_end]
        );
        let batch_code = match crate::host::compile(
            &batch_source,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
        ) {
            Ok(code) => code,
            Err(e) => {
                let diagnostic = compiler_diagnostic(
                    &batch_source,
                    windows::core::s!("main"),
                );
                if let Some(marker) = std::env::var_os("VICTORY_ROOTSTRING_TEST_MARKER") {
                    std::fs::write(
                        marker,
                        format!(
                            "adapter={adapter_name}\nroot_signatures_created={}\nsimple_compute_pso=created\nreadonly_load4_pso=created\nbatch_indirect_compile=failed\n{diagnostic}\n",
                            signatures.len()
                        ),
                    )
                    .expect("write batching shader failure diagnostic");
                }
                panic!("indirect batching shader compilation failed: {e:?}; {diagnostic}");
            }
        };
        let batch_root = make(&device, &batch_source)
            .expect("indirect batching root signature creation");
        let batch_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&batch_root) },
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: batch_code.as_ptr().cast(),
                BytecodeLength: batch_code.len(),
            },
            ..Default::default()
        };
        let _batch_pso: ID3D12PipelineState = unsafe {
            device
                .CreateComputePipelineState(&batch_desc)
                .expect("indirect batching compute PSO creation")
        };
        let lightprobe = include_str!("lightprobe.rs");
        const HLSL_MARK: &str = "const SOURCE: &str = r#\"";
        let source_at = lightprobe.find(HLSL_MARK).expect("lightprobe HLSL source exists");
        let source_after = &lightprobe[source_at + HLSL_MARK.len()..];
        let source_end = source_after.find("\"#;").expect("lightprobe HLSL source is closed");
        let mut hlsl = source_after[..source_end].to_owned();
        let load4_expanded = std::env::var_os("VICTORY_TEST_EXPAND_LOAD4").is_some();
        if load4_expanded {
            hlsl = hlsl
                .replace("stats.Load4(0)", "uint4(stats.Load(0), stats.Load(4), stats.Load(8), stats.Load(12))")
                .replace("output.Load4(0)", "uint4(output.Load(0), output.Load(4), output.Load(8), output.Load(12))");
        }
        let mut pipelines = 0;
        for (part, rs_marker, entry) in [
            ("EXPOSURE", "const RS_EXPOSURE: &str = \"", windows::core::s!("exposure")),
            ("LUX", "const RS_LUX: &str = \"", windows::core::s!("lux")),
        ] {
            let rs_at = lightprobe.find(rs_marker).expect("lightprobe RS exists");
            let rs_after = &lightprobe[rs_at + rs_marker.len()..];
            let rs_end = rs_after.find('"').expect("lightprobe RS is closed");
            let source = format!("#define {part}\n#define RS \"{}\"\n{hlsl}", &rs_after[..rs_end]);
            let code = match crate::host::compile(&source, entry, windows::core::s!("cs_5_1")) {
                Ok(code) => code,
                Err(e) => {
                    let diagnostic = compiler_diagnostic(&source, entry);
                    if let Some(marker) = std::env::var_os("VICTORY_ROOTSTRING_TEST_MARKER") {
                        std::fs::write(
                            marker,
                            format!(
                                "adapter={adapter_name}\nroot_signatures_created={}\nsimple_compute_pso=created\nreadonly_load4_pso=created\nbatch_indirect_pso=created\nload4_expanded={load4_expanded}\nlightprobe_part={part}\ncompile=failed\n{diagnostic}\n",
                                signatures.len()
                            ),
                        )
                        .expect("write compiler-failure diagnostic");
                    }
                    panic!("lightprobe {part} shader compilation failed: {e:?}; {diagnostic}");
                }
            };
            let root = make(&device, &source)
                .unwrap_or_else(|e| panic!("lightprobe {part} root signature failed: {e:?}"));
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: unsafe { std::mem::transmute_copy(&root) },
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            };
            let _pso: ID3D12PipelineState = unsafe {
                device
                    .CreateComputePipelineState(&desc)
                    .unwrap_or_else(|e| panic!("lightprobe {part} pipeline creation failed: {e:?}"))
            };
            pipelines += 1;
        }
        if let Some(marker) = std::env::var_os("VICTORY_ROOTSTRING_TEST_MARKER") {
            std::fs::write(
                marker,
                format!(
                    "adapter={adapter_name}\nroot_signatures_created={}\nsimple_compute_pso=created\nreadonly_load4_pso=created\nbatch_indirect_pso=created\nload4_expanded={load4_expanded}\nlightprobe_psos_created={pipelines}\n",
                    signatures.len()
                ),
            )
            .expect("write success marker");
        }
    }

    #[test]
    #[ignore = "requires a working D3D12 host device"]
    fn creates_compositor_pipeline_on_host_device() {
        let mut device: Option<ID3D12Device> = None;
        unsafe {
            D3D12CreateDevice(
                None::<&windows::core::IUnknown>,
                windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0,
                &mut device,
            )
            .expect("D3D12 device creation");
        }
        let device = device.expect("D3D12CreateDevice returned no device");
        let luid = unsafe { device.GetAdapterLuid() };
        let factory = unsafe {
            windows::Win32::Graphics::Dxgi::CreateDXGIFactory1::<
                windows::Win32::Graphics::Dxgi::IDXGIFactory4,
            >()
            .expect("DXGI factory creation")
        };
        let adapter = unsafe {
            factory
                .EnumAdapterByLuid::<windows::Win32::Graphics::Dxgi::IDXGIAdapter1>(luid)
                .expect("adapter matching the D3D12 device")
        };
        let desc = unsafe { adapter.GetDesc1().expect("adapter description") };
        let name_end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let adapter_name = String::from_utf16_lossy(&desc.Description[..name_end]);

        let compose = include_str!("compose.rs");
        const HLSL_MARK: &str = "const SOURCE: &str = r#\"";
        let source_at = compose.find(HLSL_MARK).expect("compositor HLSL exists");
        let source_after = &compose[source_at + HLSL_MARK.len()..];
        let source_end = source_after.find("\"#;").expect("compositor HLSL is closed");
        let source = &source_after[..source_end];
        let vs = crate::host::compile(source, windows::core::s!("vs"), windows::core::s!("vs_5_1"))
            .expect("compositor vertex shader compilation");
        let ps = crate::host::compile(source, windows::core::s!("ps"), windows::core::s!("ps_5_1"))
            .expect("compositor pixel shader compilation");
        let root = make(&device, source).expect("compositor root signature creation");
        let shader = |code: &[u8]| D3D12_SHADER_BYTECODE {
            pShaderBytecode: code.as_ptr().cast(),
            BytecodeLength: code.len(),
        };
        let mut pipeline_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            VS: shader(&vs),
            PS: shader(&ps),
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_NONE,
                ..Default::default()
            },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        pipeline_desc.RTVFormats[0] =
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
        pipeline_desc.BlendState.RenderTarget[0].RenderTargetWriteMask =
            D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let _pso: ID3D12PipelineState = unsafe {
            device
                .CreateGraphicsPipelineState(&pipeline_desc)
                .expect("compositor graphics PSO creation")
        };
        if let Some(marker) = std::env::var_os("VICTORY_COMPOSITOR_TEST_MARKER") {
            std::fs::write(
                marker,
                format!(
                    "adapter={adapter_name}\nvs=compiled\nps=compiled\nroot_signature=created\nfilter=FILTER_MIN_MAG_MIP_LINEAR\ngraphics_pso=created\n"
                ),
            )
            .expect("write compositor test result");
        }
    }
}
