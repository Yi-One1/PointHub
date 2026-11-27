//! Windows 防火墙：安装时自动放行手机访问，卸载时自动清理。
//!
//! 手机访问走 TCP 入站，Windows 防火墙默认拦截。防火墙规则只有管理员能改，
//! 所以无法在运行期完全静默处理；这里把唯一一次授权放到安装完成时：
//!
//! - 安装器调用 `pointhub.exe --firewall-install`，检查当前安装目录是否已放行；
//!   已放行则直接返回（原地升级不会重复弹 UAC），否则提权写入一条按程序放行的规则；
//! - 规则只对本地子网生效，程序升级、改端口都不受影响；
//! - 卸载时由 `purge_all` 提权删除全部 PointHub 规则。
//!
//! 便携版没有安装器，由 Windows 首次监听时的原生弹窗处理；应用内不再提示。

#[cfg(windows)]
const RULE_NAME: &str = "PointHub LAN";

/// 安装完成时调用：确保当前安装路径已有放行规则。
///
/// `quiet` 用于静默安装：没有交互界面，无法弹 UAC，只能跳过。
#[cfg(windows)]
pub fn install(quiet: bool) -> Result<(), String> {
    platform::install(quiet)
}

/// 卸载时删除全部 PointHub 防火墙规则。
#[cfg(windows)]
pub fn purge_all(quiet: bool) -> Result<(), String> {
    platform::purge_all(quiet)
}

#[cfg(not(windows))]
pub fn install(_quiet: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn purge_all(_quiet: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// 发布版没有控制台；子进程也不该闪黑框。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// PowerShell 单引号字符串里，单引号本身要写两次。
    fn ps_quote(s: &str) -> String {
        s.replace('\'', "''")
    }

    fn current_exe() -> String {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    fn run_powershell(script: &str) -> Result<String, String> {
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("无法启动 PowerShell：{e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim();
            return Err(if err.is_empty() {
                format!("PowerShell 退出码 {:?}", out.status.code())
            } else {
                err.to_string()
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// 以管理员身份跑一段 PowerShell；用户取消 UAC 时返回明确提示。
    fn run_elevated(script: &str) -> Result<(), String> {
        let encoded = utf16le_base64(script);
        let outer = format!(
            "try {{ $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru \
             -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}'; \
             exit $p.ExitCode }} catch {{ exit 1223 }}"
        );
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &outer,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("无法启动提权进程：{e}"))?;
        let code = out.status.code().unwrap_or(-1);
        if code == 0 {
            return Ok(());
        }
        if code == 1223 {
            return Err("已取消：修改防火墙需要管理员权限".into());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() {
            format!("操作失败（退出码 {code}）")
        } else {
            format!("操作失败（退出码 {code}）：{err}")
        })
    }

    fn utf16le_base64(s: &str) -> String {
        use base64::prelude::{Engine as _, BASE64_STANDARD};
        let bytes: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        BASE64_STANDARD.encode(bytes)
    }

    /// 只读检查：当前 exe 是否已有启用中的入站放行规则。
    fn has_rule_script() -> String {
        let exe = ps_quote(&current_exe());
        format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8\n\
             $exe='{exe}'\n\
             $ok=$false\n\
             foreach($r in @(Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue)){{\n\
               $enabled = ($r.Enabled -eq 'True') -and ($r.Direction -eq 'Inbound') -and ($r.Action -eq 'Allow')\n\
               if(-not $enabled){{ continue }}\n\
               $app=$r | Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue\n\
               if($app.Program -and ($app.Program -ieq $exe)){{ $ok=$true; break }}\n\
             }}\n\
             if($ok){{'YES'}}else{{'NO'}}"
        )
    }

    /// 提权脚本：清掉所有历史 PointHub 规则，再给当前 exe 建一条本地子网规则。
    fn install_script() -> String {
        let exe = ps_quote(&current_exe());
        format!(
            "$ErrorActionPreference='Stop'\n\
             $ProgressPreference='SilentlyContinue'\n\
             $exe='{exe}'\n\
             Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue | \
             Remove-NetFirewallRule -ErrorAction SilentlyContinue\n\
             New-NetFirewallRule -DisplayName '{rule}' -Direction Inbound -Action Allow \
             -Program $exe -Protocol TCP -Profile Domain,Private,Public \
             -RemoteAddress LocalSubnet -Enabled True | Out-Null\n\
             exit 0",
            rule = RULE_NAME
        )
    }

    fn purge_script() -> &'static str {
        "Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue | \
         Remove-NetFirewallRule -ErrorAction SilentlyContinue\n\
         exit 0"
    }

    pub fn install(quiet: bool) -> Result<(), String> {
        if has_rule()? {
            return Ok(());
        }
        if quiet {
            return Err("静默安装无法提权，跳过防火墙规则安装".into());
        }
        run_elevated(&install_script())
    }

    pub fn purge_all(quiet: bool) -> Result<(), String> {
        let count = count_rules()?;
        if count == 0 {
            return Ok(());
        }
        if quiet {
            return Err(format!(
                "静默卸载无法提权，跳过 {count} 条防火墙规则清理"
            ));
        }
        run_elevated(purge_script())
    }

    fn has_rule() -> Result<bool, String> {
        Ok(run_powershell(&has_rule_script())?.trim().eq_ignore_ascii_case("YES"))
    }

    fn count_rules() -> Result<usize, String> {
        let out = run_powershell(
            "@(Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue).Count",
        )?;
        out.trim()
            .parse::<usize>()
            .map_err(|_| "无法读取防火墙规则数量".to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn install_rule_is_program_scoped_and_local_subnet() {
            let s = install_script();
            assert!(s.contains("-Program $exe"), "必须绑定当前程序");
            assert!(s.contains("LocalSubnet"), "必须限制为本地子网");
            assert!(s.contains("Domain,Private,Public"));
            assert!(s.contains("Direction Inbound"));
            assert!(s.contains("Action Allow"));
        }

        #[test]
        fn install_clears_old_point_hub_rules_first() {
            let s = install_script();
            assert!(s.contains("DisplayName 'PointHub*'"));
            assert!(s.contains("Remove-NetFirewallRule"));
        }

        #[test]
        fn has_rule_checks_current_exe() {
            let s = has_rule_script();
            assert!(s.contains("-ieq $exe"));
            assert!(s.contains("PointHub*"));
            assert!(s.contains("YES"));
        }

        #[test]
        fn utf16_base64_roundtrip() {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let text = "Write-Host 中文";
            let encoded = utf16le_base64(text);
            let bytes = BASE64_STANDARD.decode(encoded).unwrap();
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            assert_eq!(String::from_utf16(&units).unwrap(), text);
        }

        #[test]
        fn ps_quote_doubles_single_quote() {
            assert_eq!(ps_quote("a'b"), "a''b");
        }
    }
}
