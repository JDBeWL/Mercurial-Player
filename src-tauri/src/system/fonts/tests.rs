//! fonts 模块单元测试(二进制解析、命名约定、提取)。

use std::collections::HashSet;
#[cfg(target_os = "windows")]
use std::fs;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

use super::parse::{
    CollectionMemberMeta, be_u16, be_u32, extract_collection_member,
    frontend_family_from_file_name, internal_font_families, member_file_name,
};
#[cfg(target_os = "windows")]
use super::scan::extract_font_name;

/// 构造最小 sfnt 字体字节（表数据任意），base 为该字体在集合文件中的绝对偏移
fn build_sfnt(base: usize, tables: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let num = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 1, 0, 0]);
    out.extend_from_slice(&(num as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // searchRange/entrySelector/rangeShift 占位
    let records_start = out.len();
    out.resize(records_start + num * 16, 0);
    for (i, &(tag, data)) in tables.iter().enumerate() {
        let offset = base + out.len();
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let rec = records_start + i * 16;
        out[rec..rec + 4].copy_from_slice(&tag);
        out[rec + 4..rec + 8].copy_from_slice(&[0xAB; 4]); // checkSum 占位
        out[rec + 8..rec + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[rec + 12..rec + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
    }
    out
}

/// 将多个成员的表目录包装成 ttcf 集合文件
fn build_collection(members: &[&[([u8; 4], &[u8])]]) -> Vec<u8> {
    let header_len = 12 + 4 * members.len();
    let mut bases = Vec::with_capacity(members.len());
    let mut cur = header_len;
    for m in members {
        let payload: usize = m.iter().map(|(_, d)| (d.len() + 3) & !3).sum();
        bases.push(cur);
        cur += 12 + m.len() * 16 + payload;
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&[0, 1, 0, 0]);
    out.extend_from_slice(&(members.len() as u32).to_be_bytes());
    for b in &bases {
        out.extend_from_slice(&(*b as u32).to_be_bytes());
    }
    for (m, b) in members.iter().zip(&bases) {
        out.extend_from_slice(&build_sfnt(*b, m));
    }
    out
}

/// 读取提取结果中指定 tag 的表数据
fn table_bytes(out: &[u8], tag: [u8; 4]) -> (usize, Vec<u8>) {
    let num = be_u16(out, 4).unwrap() as usize;
    for i in 0..num {
        let rec = 12 + i * 16;
        if out[rec..rec + 4] == tag {
            let offset = be_u32(out, rec + 8).unwrap() as usize;
            let len = be_u32(out, rec + 12).unwrap() as usize;
            return (offset, out[offset..offset + len].to_vec());
        }
    }
    panic!("表 {tag:?} 不存在");
}

#[test]
fn extract_member_roundtrip() {
    let member0: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"beta"), (*b"NAM1", b"alpha")];
    let member1: &[([u8; 4], &[u8])] = &[(*b"CMAP", b"gamma!"), (*b"HEA1", b"beta-longer")];
    let data = build_collection(&[member0, member1]);

    for (index, expected) in [
        (0usize, vec![b"HEA1", b"NAM1"]),
        (1, vec![b"CMAP", b"HEA1"]),
    ] {
        let out = extract_collection_member(&data, index).unwrap();
        // 输出是独立 sfnt 而非集合
        assert_eq!(&out[0..4], &[0, 1, 0, 0]);
        assert_eq!(be_u16(&out, 4).unwrap() as usize, expected.len());
        // 各表数据与偏移对齐保持正确
        for tag in expected {
            let (offset, bytes) = table_bytes(&out, *tag);
            assert_eq!(offset % 4, 0, "表 {tag:?} 未按 4 字节对齐");
            assert!(!bytes.is_empty());
        }
    }

    // 成员 1 的表内容与源一致，校验和占位被保留
    let out = extract_collection_member(&data, 1).unwrap();
    assert_eq!(table_bytes(&out, *b"CMAP").1, b"gamma!");
    assert_eq!(table_bytes(&out, *b"HEA1").1, b"beta-longer");
    let num = be_u16(&out, 4).unwrap() as usize;
    for i in 0..num {
        assert_eq!(&out[12 + i * 16 + 4..12 + i * 16 + 8], &[0xAB; 4]);
    }
}

#[test]
fn frontend_family_from_file_name_conventions() {
    // 与前端 bundledFonts.test.ts 的解析用例保持一致
    assert_eq!(
        frontend_family_from_file_name("Noto Sans SC-VF.woff2"),
        "Noto Sans SC"
    );
    assert_eq!(
        frontend_family_from_file_name("975Maru SC.ttf"),
        "975Maru SC"
    );
    // Google Fonts 命名（英文字重名）与 Adobe 数字字重命名
    assert_eq!(
        frontend_family_from_file_name("SourceHanSansSC-Bold.otf"),
        "SourceHanSansSC"
    );
    assert_eq!(frontend_family_from_file_name("Family-700.ttf"), "Family");
    assert_eq!(
        frontend_family_from_file_name("Roboto-Regular.ttf"),
        "Roboto"
    );
    // 数字字重优先于英文字重名；引号剔除
    assert_eq!(frontend_family_from_file_name("X-300.ttf"), "X");
    assert_eq!(frontend_family_from_file_name("My'Font.ttf"), "MyFont");
    // -VF 判定以原始文件名结尾为准：字重后缀后的 -VF 不剔除
    assert_eq!(frontend_family_from_file_name("X-VF-700.ttf"), "X-VF");
    assert_eq!(frontend_family_from_file_name("X-700-VF.ttf"), "X-700");
    // 无后缀 / 无扩展名
    assert_eq!(frontend_family_from_file_name("Any Font.ttf"), "Any Font");
    assert_eq!(frontend_family_from_file_name("NoExt"), "NoExt");
}

#[test]
fn internal_font_families_rejects_unparseable() {
    // fixture 表数据是任意的，Face::parse 无法解析 → 返回空
    let member: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"x")];
    let single = build_sfnt(0, member);
    assert!(internal_font_families(&single).is_empty());
    assert!(internal_font_families(b"garbage").is_empty());
}

#[test]
fn extract_rejects_invalid_input() {
    assert!(extract_collection_member(&[], 0).is_none());
    assert!(extract_collection_member(b"not a font at all..", 0).is_none());
    let member: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"x")];
    let data = build_collection(&[member]);
    assert!(extract_collection_member(&data, 1).is_none());
}

#[test]
fn member_file_name_conventions() {
    let mut used = HashSet::new();
    let regular = CollectionMemberMeta {
        family: "Source Han Sans SC".to_string(),
        weight: 700,
        variable: false,
        cff: false,
    };
    assert_eq!(
        member_file_name(&regular, &mut used),
        "Source Han Sans SC-700.ttf"
    );

    let vf = CollectionMemberMeta {
        family: "My Font".to_string(),
        weight: 400,
        variable: true,
        cff: true,
    };
    assert_eq!(member_file_name(&vf, &mut used), "My Font-VF.otf");

    // 非法字符与引号剔除
    let dirty = CollectionMemberMeta {
        family: "A:B*c?\"d'e".to_string(),
        weight: 400,
        variable: false,
        cff: false,
    };
    assert_eq!(member_file_name(&dirty, &mut used), "ABcde-400.ttf");

    // 同族同字重去重
    let mut used2 = HashSet::new();
    assert_eq!(
        member_file_name(&regular, &mut used2),
        "Source Han Sans SC-700.ttf"
    );
    assert_eq!(
        member_file_name(&regular, &mut used2),
        "Source Han Sans SC-700-2.ttf"
    );
}

/// 用系统自带真实 TTC（微软雅黑）验证提取结果可被正常解析。
/// 环境中不存在该文件时跳过；同时写出提取文件供外部工具复核
#[test]
#[cfg(target_os = "windows")]
fn extract_real_system_ttc() {
    let path = PathBuf::from("C:\\Windows\\Fonts\\msyh.ttc");
    let Ok(data) = fs::read(&path) else {
        return;
    };
    let out = extract_collection_member(&data, 0).expect("提取 msyh.ttc 成员 0 失败");
    let face = ttf_parser::Face::parse(&out, 0).expect("提取结果无法被 ttf-parser 解析");
    assert!(face.number_of_glyphs() > 1000, "字形数量异常");
    let family = super::parse::face_family_name(&face).expect("读取族名失败");
    assert!(
        family.contains("YaHei") || family.contains("雅黑"),
        "族名异常: {family}"
    );

    let families = internal_font_families(&data);
    assert!(!families.is_empty(), "真实 TTC 应解析出内部族名");

    let dump = std::env::temp_dir().join("mercurial-player-test-msyh-member0.ttf");
    fs::write(&dump, &out).expect("写出提取结果失败");
}

#[test]
#[cfg(target_os = "windows")]
fn test_extract_font_name() {
    assert_eq!(
        extract_font_name("Arial (TrueType)"),
        Some("Arial".to_string())
    );

    assert_eq!(
        extract_font_name("Microsoft YaHei & Microsoft YaHei UI (TrueType)"),
        Some("Microsoft YaHei".to_string())
    );

    assert_eq!(
        extract_font_name("Segoe UI Bold (TrueType)"),
        Some("Segoe UI".to_string())
    );

    assert_eq!(
        extract_font_name("Consolas (TrueType)"),
        Some("Consolas".to_string())
    );
}
