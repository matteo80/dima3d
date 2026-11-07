use std::collections::HashMap;
use crate::parser::lexer::{scan_word, scan_numbers_f32, skip_whitespace_comments};

/// Costruisce la matrice di trasformazione VRML per un nodo `Transform`.
///
/// Formula VRML 2.0:
///   M = T * T_c * R * R_so * S * R_so_inv * T_neg_c
///
/// dove:
///   T         = translation
///   T_c       = center
///   R         = rotation (asse-angolo)
///   R_so      = scaleOrientation (asse-angolo)
///   S         = scale
///   T_neg_c   = -center
pub fn build_transform_matrix(
    data: &[u8],
    block_start: usize,
    block_end: usize,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
) -> glam::Mat4 {
    // Valori VRML default
    let mut translation = glam::Vec3::ZERO;
    let mut rotation_axis = glam::Vec3::Y;
    let mut rotation_angle: f32 = 0.0;
    let mut scale = glam::Vec3::ONE;
    let mut center = glam::Vec3::ZERO;
    let mut scale_orientation_axis = glam::Vec3::Z;
    let mut scale_orientation_angle: f32 = 0.0;

    let mut pos = block_start;

    while pos < block_end {
        pos = skip_whitespace_comments(data, pos);
        if pos >= block_end {
            break;
        }

        // In VRML i campi possono stare in qualsiasi ordine, anche dopo `children` (alcuni CAD
        // scrivono `scale` in fondo): i blocchi dei figli si saltano senza leggerne il contenuto.
        let closing = match data[pos] {
            b'{' => brace_map.get(&pos),
            b'[' => bracket_map.get(&pos),
            _ => None,
        };
        if let Some(&close) = closing {
            pos = close + 1;
            continue;
        }

        let Some((word, next_pos)) = scan_word(data, pos) else {
            break;
        };

        match word {
            // Il nome che segue può essere qualsiasi parola, anche "scale"
            "DEF" | "USE" => {
                let name = skip_whitespace_comments(data, next_pos);
                pos = scan_word(data, name).map_or(next_pos, |(_, after)| after);
            }
            "translation" => {
                let nums = scan_numbers_f32(data, next_pos, next_pos + 64);
                if nums.len() >= 3 {
                    translation = glam::Vec3::new(nums[0], nums[1], nums[2]);
                }
                pos = advance_past_values(data, next_pos, block_end, 3);
            }
            "rotation" => {
                let nums = scan_numbers_f32(data, next_pos, next_pos + 80);
                if nums.len() >= 4 {
                    let ax = glam::Vec3::new(nums[0], nums[1], nums[2]);
                    rotation_axis = if ax.length_squared() > 1e-10 {
                        ax.normalize()
                    } else {
                        glam::Vec3::Y
                    };
                    rotation_angle = nums[3];
                }
                pos = advance_past_values(data, next_pos, block_end, 4);
            }
            "scale" => {
                let nums = scan_numbers_f32(data, next_pos, next_pos + 64);
                if nums.len() >= 3 {
                    scale = glam::Vec3::new(nums[0], nums[1], nums[2]);
                }
                pos = advance_past_values(data, next_pos, block_end, 3);
            }
            "center" => {
                let nums = scan_numbers_f32(data, next_pos, next_pos + 64);
                if nums.len() >= 3 {
                    center = glam::Vec3::new(nums[0], nums[1], nums[2]);
                }
                pos = advance_past_values(data, next_pos, block_end, 3);
            }
            "scaleOrientation" => {
                let nums = scan_numbers_f32(data, next_pos, next_pos + 80);
                if nums.len() >= 4 {
                    let ax = glam::Vec3::new(nums[0], nums[1], nums[2]);
                    scale_orientation_axis = if ax.length_squared() > 1e-10 {
                        ax.normalize()
                    } else {
                        glam::Vec3::Z
                    };
                    scale_orientation_angle = nums[3];
                }
                pos = advance_past_values(data, next_pos, block_end, 4);
            }
            _ => {
                pos = next_pos;
            }
        }
    }

    // Costruzione della matrice secondo la specifica VRML 2.0
    let t = glam::Mat4::from_translation(translation);
    let t_c = glam::Mat4::from_translation(center);
    let t_neg_c = glam::Mat4::from_translation(-center);
    let r = glam::Mat4::from_axis_angle(rotation_axis, rotation_angle);
    let r_so = glam::Mat4::from_axis_angle(scale_orientation_axis, scale_orientation_angle);
    let r_so_inv = glam::Mat4::from_axis_angle(scale_orientation_axis, -scale_orientation_angle);
    let s = glam::Mat4::from_scale(scale);

    // M = T * T_c * R * R_so * S * R_so_inv * T_neg_c
    t * t_c * r * r_so * s * r_so_inv * t_neg_c
}

/// Avanza la posizione oltre `count` valori numerici nel buffer.
/// Usato per skippare i valori già letti.
fn advance_past_values(data: &[u8], start: usize, end: usize, count: usize) -> usize {
    let mut pos = start;
    let mut found = 0;
    while pos < end && found < count {
        pos = skip_whitespace_comments(data, pos);
        if pos >= end {
            break;
        }
        // Salta un token numerico
        let b = data[pos];
        if b == b'-' || b == b'+' || b.is_ascii_digit() || b == b'.' {
            while pos < end {
                let c = data[pos];
                if c.is_ascii_whitespace() || c == b',' {
                    break;
                }
                pos += 1;
            }
            found += 1;
        } else {
            pos += 1;
        }
    }
    pos
}
