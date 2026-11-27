use crate::{boolean, geometry, reference, sized, string_index, Mesh};
use fo4_core::{add, limit, mul, range, Budget, Cursor, Error, Limits, Result};
use serde::Serialize;
use std::ops::Range;
#[derive(Debug, Clone, Serialize)]
pub enum Value {
    U32(u32),
    U64(u64),
    U16(u16),
    U8(u8),
    FloatBits(u32),
    Ref(i32),
    StringIndex(u32),
    Bytes(Vec<u8>),
    Raw(Range<usize>),
}
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub offset: u64,
    pub value: Value,
}
#[derive(Debug, Clone, Serialize)]
pub enum Data {
    Node {
        fields: Vec<Field>,
        children: Vec<i32>,
    },
    Mesh(Mesh),
    Fields(Vec<Field>),
}
pub(crate) struct Reader<'a, 's> {
    pub c: &'a mut Cursor<'s>,
    pub bs: u32,
    pub strings: &'a [Vec<u8>],
    pub blocks: usize,
    pub l: &'a Limits,
    pub b: &'a Budget,
    pub fields: Vec<Field>,
}
impl Reader<'_, '_> {
    pub fn put(&mut self, name: &'static str, o: u64, value: Value) -> Result<()> {
        self.b
            .reserve(&mut self.fields, 1, self.c.source.name(), o)?;
        self.fields.push(Field {
            name,
            offset: o,
            value,
        });
        Ok(())
    }
    pub fn u32(&mut self, n: &'static str) -> Result<u32> {
        let o = self.c.pos;
        let v = self.c.u32()?;
        self.put(n, o, Value::U32(v))?;
        Ok(v)
    }
    pub fn u16(&mut self, n: &'static str) -> Result<u16> {
        let o = self.c.pos;
        let v = self.c.u16()?;
        self.put(n, o, Value::U16(v))?;
        Ok(v)
    }
    pub fn u8(&mut self, n: &'static str) -> Result<u8> {
        let o = self.c.pos;
        let v = self.c.u8()?;
        self.put(n, o, Value::U8(v))?;
        Ok(v)
    }
    pub fn float(&mut self, n: &'static str) -> Result<f32> {
        let o = self.c.pos;
        let v = self.c.u32()?;
        self.put(n, o, Value::FloatBits(v))?;
        Ok(f32::from_bits(v))
    }
    pub fn floats(&mut self, n: &'static str, count: u64) -> Result<()> {
        self.check_array(count, 4)?;
        for _ in 0..count {
            self.float(n)?;
        }
        Ok(())
    }
    pub fn bool(&mut self, n: &'static str) -> Result<bool> {
        let o = self.c.pos;
        let v = boolean(self.c)?;
        self.put(n, o, Value::U8(v as u8))?;
        Ok(v)
    }
    pub fn link(&mut self, n: &'static str) -> Result<i32> {
        let o = self.c.pos;
        let v = reference(self.c, self.blocks)?;
        self.put(n, o, Value::Ref(v))?;
        Ok(v)
    }
    pub fn string(&mut self, n: &'static str) -> Result<u32> {
        let o = self.c.pos;
        let v = string_index(self.c, self.strings)?;
        self.put(n, o, Value::StringIndex(v))?;
        Ok(v)
    }
    pub fn text(&mut self, n: &'static str) -> Result<()> {
        let o = self.c.pos;
        let bytes = sized(self.c, self.l, self.b)?;
        self.put(n, o, Value::Bytes(bytes))
    }
    pub fn raw(&mut self, n: &'static str, size: u64) -> Result<()> {
        let o = self.c.pos;
        range(o, size, self.c.end, self.c.source.name())?;
        self.c.pos = add(o, size, self.c.source.name(), o)?;
        self.put(n, o, Value::Raw(o as usize..self.c.pos as usize))
    }
    pub fn check_array(&self, count: u64, unit: u64) -> Result<()> {
        limit(count, self.l.max_entries, self.c.source.name(), self.c.pos)?;
        range(
            self.c.pos,
            mul(count, unit, self.c.source.name(), self.c.pos)?,
            self.c.end,
            self.c.source.name(),
        )
    }
    pub fn net(&mut self, shader: bool) -> Result<Option<u32>> {
        let shader = if shader {
            Some(self.u32("Shader Type")?)
        } else {
            None
        };
        self.string("Name")?;
        let n = self.u32("Num Extra Data")?;
        self.check_array(n as u64, 4)?;
        for _ in 0..n {
            self.link("Extra Data")?;
        }
        self.link("Controller")?;
        Ok(shader)
    }
    pub fn av(&mut self) -> Result<()> {
        self.net(false)?;
        self.u32("Flags")?;
        self.floats("Translation", 3)?;
        self.floats("Rotation", 9)?;
        self.float("Scale")?;
        self.link("Collision")?;
        Ok(())
    }
}
pub(crate) fn read(
    name: &str,
    c: &mut Cursor<'_>,
    bs: u32,
    strings: &[Vec<u8>],
    blocks: usize,
    l: &Limits,
    b: &Budget,
) -> Result<Option<Data>> {
    let mut r = Reader {
        c,
        bs,
        strings,
        blocks,
        l,
        b,
        fields: Vec::new(),
    };
    match name {
        "NiNode" | "BSFadeNode" | "BSLeafAnimNode" | "NiBillboardNode" | "NiSwitchNode"
        | "BSMultiBoundNode" | "BSOrderedNode" | "BSRangeNode" | "BSValueNode" | "BSBlastNode"
        | "BSDamageStage" | "BSTreeNode" => {
            r.av()?;
            let count = r.u32("Num Children")?;
            r.check_array(count as u64, 4)?;
            let mut children = Vec::new();
            b.reserve(&mut children, count as usize, r.c.source.name(), r.c.pos)?;
            for _ in 0..count {
                children.push(reference(r.c, blocks)?);
            }
            if bs < 130 {
                let n = r.u32("Num Effects")?;
                r.check_array(n as u64, 4)?;
                for _ in 0..n {
                    r.link("Effect")?;
                }
            }
            match name {
                "NiBillboardNode" => {
                    r.u16("Billboard Mode")?;
                }
                "NiSwitchNode" => {
                    r.u16("Switch Flags")?;
                    r.u32("Active Child")?;
                }
                "BSMultiBoundNode" => {
                    r.link("Multi Bound")?;
                    r.u32("Culling Mode")?;
                }
                "BSOrderedNode" => {
                    r.floats("Alpha Sort Bound", 4)?;
                    r.bool("Static Bound")?;
                }
                "BSRangeNode" | "BSBlastNode" | "BSDamageStage" => {
                    r.u8("Min")?;
                    r.u8("Max")?;
                    r.u8("Current")?;
                }
                "BSValueNode" => {
                    r.u32("Value")?;
                    r.u8("Value Node Flags")?;
                }
                "BSTreeNode" => {
                    for _ in 0..2 {
                        let n = r.u32("Num Bones")?;
                        r.check_array(n as u64, 4)?;
                        for _ in 0..n {
                            r.link("Bone")?;
                        }
                    }
                }
                _ => {}
            }
            Ok(Some(Data::Node {
                fields: r.fields,
                children,
            }))
        }
        "BSTriShape" | "BSSubIndexTriShape" | "BSMeshLODTriShape" | "BSDynamicTriShape" => {
            geometry::compact(name, r).map(|m| Some(Data::Mesh(m)))
        }
        "NiTriShape" => {
            r.av()?;
            r.link("Data")?;
            r.link("Skin Instance")?;
            let n = r.u32("Num Materials")?;
            r.check_array(n as u64, 8)?;
            for _ in 0..n {
                r.string("Material Name")?;
            }
            for _ in 0..n {
                r.u32("Material Extra")?;
            }
            r.u32("Active Material")?;
            r.bool("Material Needs Update")?;
            r.link("Shader Property")?;
            r.link("Alpha Property")?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiTriShapeData" => geometry::legacy(r).map(|m| Some(Data::Mesh(m))),
        // nifxml NiCollisionObject + bhkNPCollisionObject, versions #FO4# #F76#.
        "bhkNPCollisionObject" if bs == 130 => {
            r.link("Target")?;
            r.u16("Flags")?;
            r.link("Data")?;
            r.u32("Body ID")?;
            Ok(Some(Data::Fields(r.fields)))
        }
        // nifxml bhkSystem subclasses: one ByteArray. The bytes stay opaque.
        "bhkPhysicsSystem" | "bhkRagdollSystem" if bs == 130 => {
            let size = r.u32("Data Size")?;
            r.raw("Binary Data", size as u64)?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "BSPackedCombinedSharedGeomDataExtra" => {
            r.string("Name")?;
            let o = r.c.pos;
            let desc = r.c.u64()?;
            r.put("Vertex Desc", o, Value::U64(desc))?;
            r.u32("Num Vertices")?;
            r.u32("Num Triangles")?;
            r.raw("Unknown Flags 1", 4)?;
            r.raw("Unknown Flags 2", 4)?;
            let n = r.u32("Num Data")?;
            r.check_array(n as u64, 8)?;
            for _ in 0..n {
                r.u32("Filename Hash")?;
                r.u32("Data Offset")?;
            }
            r.check_array(n as u64, 44)?;
            for _ in 0..n {
                for field in [
                    "Num Verts",
                    "LOD Levels",
                    "Tri Count LOD0",
                    "Tri Offset LOD0",
                    "Tri Count LOD1",
                    "Tri Offset LOD1",
                    "Tri Count LOD2",
                    "Tri Offset LOD2",
                ] {
                    r.u32(field)?;
                }
                let combined = r.u32("Num Combined")?;
                r.check_array(combined as u64, 72)?;
                for _ in 0..combined {
                    r.float("Grayscale to Palette Scale")?;
                    r.floats("Combined Rotation", 9)?;
                    r.floats("Combined Translation", 3)?;
                    r.float("Combined Scale")?;
                    r.floats("Combined Bounding Sphere", 4)?;
                }
                let o = r.c.pos;
                let desc = r.c.u64()?;
                r.put("Combined Vertex Desc", o, Value::U64(desc))?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "BSConnectPoint::Parents"
        | "BSConnectPoint::Children"
        | "BSEyeCenterExtraData"
        | "BSBoneLODExtraData"
        | "BSFurnitureMarker"
        | "BSFurnitureMarkerNode" => {
            r.string("Name")?;
            if name == "BSConnectPoint::Children" {
                r.bool("Skinned")?;
            }
            let n = r.u32("Count")?;
            r.check_array(n as u64, 1)?;
            for _ in 0..n {
                match name {
                    "BSConnectPoint::Parents" => {
                        r.text("Parent")?;
                        r.text("Name")?;
                        r.floats("Rotation", 4)?;
                        r.floats("Translation", 3)?;
                        r.float("Scale")?;
                    }
                    "BSConnectPoint::Children" => r.text("Point Name")?,
                    "BSEyeCenterExtraData" => {
                        r.float("Data")?;
                    }
                    "BSBoneLODExtraData" => {
                        r.u32("Distance")?;
                        r.string("Bone Name")?;
                    }
                    _ => {
                        r.floats("Offset", 3)?;
                        r.float("Heading")?;
                        r.u16("Animation Type")?;
                        r.u16("Entry Properties")?;
                    }
                }
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiControllerManager" | "NiMultiTargetTransformController" => {
            read_timing(&mut r)?;
            if name == "NiControllerManager" {
                r.bool("Cumulative")?;
                let n = r.u32("Num Controller Sequences")?;
                r.check_array(n as u64, 4)?;
                for _ in 0..n {
                    r.link("Controller Sequence")?;
                }
                r.link("Object Palette")?;
            } else {
                let n = r.u16("Num Extra Targets")?;
                r.check_array(n as u64, 4)?;
                for _ in 0..n {
                    r.link("Extra Target")?;
                }
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiControllerSequence" => {
            r.string("Name")?;
            let n = r.u32("Num Controlled Blocks")?;
            r.u32("Array Grow By")?;
            r.check_array(n as u64, 29)?;
            for _ in 0..n {
                r.link("Interpolator")?;
                r.link("Controller")?;
                r.u8("Priority")?;
                for label in [
                    "Node Name",
                    "Property Type",
                    "Controller Type",
                    "Controller ID",
                    "Interpolator ID",
                ] {
                    r.string(label)?;
                }
            }
            r.float("Weight")?;
            r.link("Text Keys")?;
            r.u32("Cycle Type")?;
            r.float("Frequency")?;
            r.float("Start Time")?;
            r.float("Stop Time")?;
            r.link("Manager")?;
            r.string("Accum Root Name")?;
            let notes = r.u16("Num Anim Note Arrays")?;
            r.check_array(notes as u64, 4)?;
            for _ in 0..notes {
                r.link("Anim Note Array")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiDefaultAVObjectPalette" => {
            r.link("Scene")?;
            let n = r.u32("Num Objs")?;
            r.check_array(n as u64, 8)?;
            for _ in 0..n {
                r.text("Name")?;
                r.link("AV Object")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiParticleSystem" => {
            // Bethesda 100/130 inheritance shift to BSGeometry, not legacy NiGeometry.
            r.av()?;
            r.floats("Bounding Sphere", 4)?;
            r.link("Skin")?;
            r.link("Shader Property")?;
            r.link("Alpha Property")?;
            let offset = r.c.pos;
            let desc = r.c.u64()?;
            r.put("Vertex Desc", offset, Value::U64(desc))?;
            for label in ["Far Begin", "Far End", "Near Begin", "Near End"] {
                r.u16(label)?;
            }
            r.link("Data")?;
            r.bool("World Space")?;
            let count = r.u32("Num Modifiers")?;
            r.check_array(count as u64, 4)?;
            for _ in 0..count {
                r.link("Modifier")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiPSysEmitterCtlr" | "NiPSysModifierActiveCtlr" => {
            read_timing(&mut r)?;
            r.link("Interpolator")?;
            r.string("Modifier Name")?;
            if name == "NiPSysEmitterCtlr" {
                r.link("Visibility Interpolator")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiFloatData" | "NiPosData" | "NiBoolData" => {
            read_keys(
                &mut r,
                if name == "NiPosData" { 3 } else { 1 },
                name == "NiBoolData",
            )?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiTransformData" => {
            let count = r.u32("Num Rotation Keys")?;
            r.check_array(count as u64, 0)?;
            if count != 0 {
                let offset = r.c.pos;
                let kind = r.u32("Rotation Type")?;
                if kind == 4 {
                    if count != 1 {
                        return Err(Error::new(
                            "InvalidXyzRotationKeyCount",
                            r.c.source.name(),
                            offset - 4,
                            1,
                            count,
                        ));
                    }
                    for _ in 0..3 {
                        read_keys(&mut r, 1, false)?;
                    }
                } else {
                    check_key_type(&r, kind, offset)?;
                    r.check_array(count as u64, if kind == 3 { 32 } else { 20 })?;
                    for _ in 0..count {
                        r.float("Key Time")?;
                        r.floats("Quaternion Value", 4)?;
                        if kind == 3 {
                            r.floats("Tension Bias Continuity", 3)?;
                        }
                    }
                }
            }
            read_keys(&mut r, 3, false)?;
            read_keys(&mut r, 1, false)?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiBlendFloatInterpolator" | "NiBlendPoint3Interpolator" => {
            let offset = r.c.pos;
            let flags = r.u8("Blend Flags")?;
            if flags & !3 != 0 {
                return Err(Error::new(
                    "UnsupportedBlendFlags",
                    r.c.source.name(),
                    offset,
                    "only documented bits 0..1",
                    flags,
                ));
            }
            let count = r.u8("Array Size")?;
            r.float("Weight Threshold")?;
            if flags & 1 == 0 {
                r.u8("Interp Count")?;
                r.u8("Single Index")?;
                r.u8("High Priority Raw Signed Byte")?;
                r.u8("Next High Priority Raw Signed Byte")?;
                for label in [
                    "Single Time",
                    "High Weights Sum",
                    "Next High Weights Sum",
                    "High Ease Spinner",
                ] {
                    r.float(label)?;
                }
                r.check_array(count as u64, 17)?;
                for _ in 0..count {
                    r.link("Interpolator")?;
                    r.float("Weight")?;
                    r.float("Normalized Weight")?;
                    r.u8("Priority")?;
                    r.float("Ease Spinner")?;
                }
            }
            r.floats(
                "Pose Value",
                if name == "NiBlendPoint3Interpolator" {
                    3
                } else {
                    1
                },
            )?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiTransformInterpolator"
        | "NiFloatInterpolator"
        | "NiPoint3Interpolator"
        | "NiBoolInterpolator"
        | "NiBoolTimelineInterpolator" => {
            match name {
                "NiTransformInterpolator" => {
                    r.floats("Translation", 3)?;
                    r.floats("Quaternion", 4)?;
                    r.float("Scale")?;
                }
                "NiFloatInterpolator" => {
                    r.float("Value")?;
                }
                "NiPoint3Interpolator" => r.floats("Value", 3)?,
                _ => {
                    r.u8("Value or invalid sentinel")?;
                }
            }
            r.link("Data")?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "BSShaderTextureSet" => {
            let n = r.u32("Num Textures")?;
            r.check_array(n as u64, 4)?;
            for _ in 0..n {
                r.text("Texture")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "BSLightingShaderProperty" => {
            let shader = r.net(true)?.ok_or_else(|| {
                Error::new(
                    "MissingShaderType",
                    r.c.source.name(),
                    r.c.pos,
                    "shader type",
                    "absent",
                )
            })?;
            r.u32("Shader Flags 1")?;
            r.u32("Shader Flags 2")?;
            r.floats("UV Offset", 2)?;
            r.floats("UV Scale", 2)?;
            r.link("Texture Set")?;
            r.floats("Emissive Color", 3)?;
            r.float("Emissive Multiple")?;
            if bs == 130 {
                r.string("Root Material")?;
            }
            r.u32("Texture Clamp Mode")?;
            r.float("Alpha")?;
            r.float("Refraction Strength")?;
            r.float(if bs < 130 { "Glossiness" } else { "Smoothness" })?;
            r.floats("Specular Color", 3)?;
            r.float("Specular Strength")?;
            if bs < 130 {
                r.float("Lighting Effect 1")?;
                r.float("Lighting Effect 2")?;
            } else {
                r.float("Subsurface Rolloff")?;
                let rim = r.float("Rimlight Power")?;
                if (f32::MAX..f32::INFINITY).contains(&rim) {
                    r.float("Backlight Power")?;
                }
                r.float("Grayscale to Palette Scale")?;
                r.float("Fresnel Power")?;
                for n in [
                    "Wet Spec Scale",
                    "Wet Spec Power",
                    "Wet Min Var",
                    "Wet Env Map Scale",
                    "Wet Fresnel Power",
                    "Wet Metalness",
                ] {
                    r.float(n)?;
                }
            }
            match shader {
                1 => {
                    r.float("Environment Map Scale")?;
                    if bs == 130 {
                        r.bool("Screen Space Reflections")?;
                        r.bool("Wetness Use SSR")?;
                    }
                }
                5 => {
                    r.floats("Skin Tint Color", 3)?;
                    if bs == 130 {
                        r.float("Skin Tint Alpha")?;
                    }
                }
                6 => r.floats("Hair Tint Color", 3)?,
                7 => {
                    r.float("Max Passes")?;
                    r.float("Scale")?;
                }
                11 => {
                    r.float("Parallax Inner Thickness")?;
                    r.float("Parallax Refraction Scale")?;
                    r.floats("Parallax Texture Scale", 2)?;
                    r.float("Parallax Envmap Strength")?;
                }
                14 => r.floats("Sparkle Parameters", 4)?,
                16 => {
                    r.float("Eye Cubemap Scale")?;
                    r.floats("Left Eye Reflection Center", 3)?;
                    r.floats("Right Eye Reflection Center", 3)?;
                }
                _ => {}
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "BSEffectShaderProperty" => {
            r.net(false)?;
            r.u32("Shader Flags 1")?;
            r.u32("Shader Flags 2")?;
            r.floats("UV Offset", 2)?;
            r.floats("UV Scale", 2)?;
            r.text("Source Texture")?;
            r.u8("Texture Clamp Mode")?;
            r.u8("Lighting Influence")?;
            r.u8("Env Map Min LOD")?;
            r.u8("Unused Byte")?;
            r.float("Falloff Start Angle")?;
            r.float("Falloff Stop Angle")?;
            r.float("Falloff Start Opacity")?;
            r.float("Falloff Stop Opacity")?;
            r.floats("Base Color", 4)?;
            r.float("Base Color Scale")?;
            r.float("Soft Falloff Depth")?;
            r.text("Greyscale Texture")?;
            if bs == 130 {
                r.text("Environment Map Texture")?;
                r.text("Normal Texture")?;
                r.text("Environment Mask Texture")?;
                r.float("Environment Map Scale")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiAlphaProperty" => {
            r.net(false)?;
            r.u16("Alpha Flags")?;
            r.u8("Threshold")?;
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiStringExtraData"
        | "NiIntegerExtraData"
        | "BSXFlags"
        | "NiBooleanExtraData"
        | "NiFloatExtraData"
        | "NiFloatsExtraData"
        | "NiIntegersExtraData"
        | "NiStringsExtraData"
        | "NiBinaryExtraData"
        | "BSBound"
        | "BSInvMarker"
        | "BSBehaviorGraphExtraData"
        | "NiTextKeyExtraData" => {
            r.string("Name")?;
            match name {
                "NiStringExtraData" => {
                    r.string("String")?;
                }
                "NiIntegerExtraData" | "BSXFlags" => {
                    r.u32("Integer Data")?;
                }
                "NiBooleanExtraData" => {
                    r.u8("Boolean Data")?;
                }
                "NiFloatExtraData" => {
                    r.float("Float Data")?;
                }
                "NiFloatsExtraData" | "NiIntegersExtraData" | "NiStringsExtraData" => {
                    let n = r.u32("Count")?;
                    r.check_array(n as u64, 4)?;
                    for _ in 0..n {
                        match name {
                            "NiFloatsExtraData" => {
                                r.float("Float")?;
                            }
                            "NiStringsExtraData" => {
                                r.text("String")?;
                            }
                            _ => {
                                r.u32("Integer")?;
                            }
                        }
                    }
                }
                "NiBinaryExtraData" => {
                    let n = r.u32("Num Bytes")?;
                    r.raw("Binary Data", n as u64)?;
                }
                "BSBound" => {
                    r.floats("Center", 3)?;
                    r.floats("Dimensions", 3)?;
                }
                "BSInvMarker" => {
                    r.u16("Rotation X")?;
                    r.u16("Rotation Y")?;
                    r.u16("Rotation Z")?;
                    r.float("Zoom")?;
                }
                "BSBehaviorGraphExtraData" => {
                    r.string("Behavior Graph File")?;
                    r.bool("Controls Base Skeleton")?;
                }
                "NiTextKeyExtraData" => {
                    let n = r.u32("Num Text Keys")?;
                    r.check_array(n as u64, 8)?;
                    for _ in 0..n {
                        r.float("Time")?;
                        r.string("Text")?;
                    }
                }
                _ => {}
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        "NiTransformController"
        | "NiAlphaController"
        | "NiVisController"
        | "NiFloatInterpController"
        | "BSEffectShaderPropertyFloatController"
        | "BSEffectShaderPropertyColorController"
        | "BSLightingShaderPropertyFloatController"
        | "BSLightingShaderPropertyColorController"
        | "BSNiAlphaPropertyTestRefController"
        | "BSFrustumFOVController"
        | "NiLightColorController"
        | "NiLightDimmerController"
        | "NiLightRadiusController" => {
            r.link("Next Controller")?;
            r.u16("Controller Flags")?;
            r.float("Frequency")?;
            r.float("Phase")?;
            r.float("Start Time")?;
            r.float("Stop Time")?;
            r.link("Target")?;
            r.link("Interpolator")?;
            if matches!(
                name,
                "BSEffectShaderPropertyFloatController"
                    | "BSEffectShaderPropertyColorController"
                    | "BSLightingShaderPropertyFloatController"
                    | "BSLightingShaderPropertyColorController"
            ) {
                r.u32("Controlled Variable")?;
            }
            if name == "NiLightColorController" {
                r.u16("Target Color")?;
            }
            Ok(Some(Data::Fields(r.fields)))
        }
        _ => {
            // The common NiTimeController prefix is documented independently
            // of each derived body's layout. Unknown body bytes stay explicit.
            let prefix = include_str!("../../../spec/static-blocks.tsv")
                .lines()
                .find_map(|row| {
                    let mut columns = row.split('\t');
                    if columns.next() != Some(name) {
                        return None;
                    }
                    columns
                        .nth(3)
                        .filter(|p| p.starts_with("NiTimeController26"))
                });
            if let Some(prefix) = prefix {
                read_timing(&mut r)?;
                if prefix != "NiTimeController26Full" {
                    let remaining = r.c.end.checked_sub(r.c.pos).ok_or_else(|| {
                        Error::new(
                            "Overflow",
                            r.c.source.name(),
                            r.c.pos,
                            "bounded controller body",
                            r.c.end,
                        )
                    })?;
                    r.raw("Controller Body", remaining)?;
                }
                Ok(Some(Data::Fields(r.fields)))
            } else {
                Ok(None)
            }
        }
    }
}

fn read_timing(r: &mut Reader<'_, '_>) -> Result<()> {
    r.link("Next Controller")?;
    r.u16("Controller Flags")?;
    r.float("Frequency")?;
    r.float("Phase")?;
    r.float("Start Time")?;
    r.float("Stop Time")?;
    r.link("Target")?;
    Ok(())
}

// Layout only: no interpolation, time sampling, pose fallback or runtime release.
fn check_key_type(r: &Reader<'_, '_>, kind: u32, offset: u64) -> Result<()> {
    if !matches!(kind, 1 | 2 | 3 | 5) {
        return Err(Error::new(
            "UnsupportedNifKeyType",
            r.c.source.name(),
            offset,
            "documented non-XYZ KeyType 1/2/3/5",
            kind,
        ));
    }
    Ok(())
}
fn read_keys(r: &mut Reader<'_, '_>, components: u64, byte: bool) -> Result<()> {
    let count = r.u32("Num Keys")?;
    r.check_array(count as u64, 0)?;
    if count == 0 {
        return Ok(());
    }
    let offset = r.c.pos;
    let kind = r.u32("Interpolation")?;
    check_key_type(r, kind, offset)?;
    let width = if byte { 1 } else { 4 * components };
    let unit = 4
        + width
        + if kind == 2 {
            2 * width
        } else if kind == 3 {
            12
        } else {
            0
        };
    r.check_array(count as u64, unit)?;
    for _ in 0..count {
        r.float("Key Time")?;
        for name in if kind == 2 {
            &["Key Value", "Forward Tangent", "Backward Tangent"][..]
        } else {
            &["Key Value"][..]
        } {
            if byte {
                r.u8(name)?;
            } else {
                r.floats(name, components)?;
            }
        }
        if kind == 3 {
            r.floats("Tension Bias Continuity", 3)?;
        }
    }
    Ok(())
}
