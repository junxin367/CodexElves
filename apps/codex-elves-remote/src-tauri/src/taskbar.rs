//! Windows 任务栏数字覆盖图标；不改变程序本身的图标或托盘菜单。
use std::ptr::null_mut;
use tauri::image::Image;
use windows_sys::Win32::Graphics::Gdi::*;

fn label(count: u32) -> String {
    // 用户要求显示未读会话数量；保留真实计数，仅在小尺寸任务栏图标上将三位以上显示为 99+。
    if count > 99 {
        "99+".into()
    } else {
        count.to_string()
    }
}

pub fn icon(count: u32) -> anyhow::Result<Option<Image<'static>>> {
    if count == 0 {
        return Ok(None);
    }
    const SIZE: usize = 64;
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        anyhow::ensure!(!dc.is_null(), "无法创建任务栏角标画布");
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE as i32,
                biHeight: -(SIZE as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            anyhow::bail!("无法创建任务栏角标位图");
        }
        let selected = SelectObject(dc, bitmap);
        let mut alpha = vec![0_u8; SIZE * SIZE];
        {
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, SIZE * SIZE * 4);
            for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
                let x = (index % SIZE) as f64 + 0.5 - SIZE as f64 / 2.0;
                let y = (index / SIZE) as f64 + 0.5 - SIZE as f64 / 2.0;
                let coverage =
                    ((SIZE as f64 / 2.0 - 0.5 - x.hypot(y)).clamp(0.0, 1.0) * 255.0).round() as u8;
                alpha[index] = coverage;
                // 沿用会话未读蓝色 #3076df；高分辨率绘制后由 Windows 按任务栏 DPI 缩放。
                pixel.copy_from_slice(&[223, 118, 48, coverage]);
            }
        }
        let text: Vec<u16> = label(count).encode_utf16().collect();
        let font = CreateFontW(
            if count < 10 {
                -44
            } else if count < 100 {
                -36
            } else {
                -27
            },
            0,
            0,
            0,
            FW_BOLD as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            ANTIALIASED_QUALITY as u32,
            0,
            windows_sys::core::w!("Segoe UI"),
        );
        if font.is_null() {
            SelectObject(dc, selected);
            DeleteObject(bitmap);
            DeleteDC(dc);
            anyhow::bail!("无法创建任务栏角标字体");
        }
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ff_ffff);
        let mut bounds = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: SIZE as i32,
            bottom: SIZE as i32,
        };
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut bounds,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        GdiFlush();
        let pixels = std::slice::from_raw_parts(bits as *const u8, SIZE * SIZE * 4);
        let rgba = pixels
            .chunks_exact(4)
            .enumerate()
            .flat_map(|(index, pixel)| [pixel[2], pixel[1], pixel[0], alpha[index]])
            .collect();
        SelectObject(dc, old_font);
        SelectObject(dc, selected);
        DeleteObject(font);
        DeleteObject(bitmap);
        DeleteDC(dc);
        Ok(Some(Image::new_owned(rgba, SIZE as u32, SIZE as u32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_badge_zero_removes_overlay_and_large_counts_keep_a_legible_label() {
        assert!(icon(0).unwrap().is_none());
        assert_eq!(label(1), "1");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
        assert_eq!(label(u32::MAX), "99+");
    }

    #[test]
    fn unread_badge_is_transparent_outside_the_circle_and_contains_a_white_number() {
        let image = icon(12).unwrap().unwrap();
        assert_eq!((image.width(), image.height()), (64, 64));
        assert_eq!(image.rgba()[3], 0);
        assert!(
            image
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == [255, 255, 255, 255])
        );
        assert!(
            image
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel == [48, 118, 223, 255])
        );
    }
}
