//! Windows 防火墙：检查手机访问端口的放行状态，并提供提权修复与旧规则清理。
//!
//! 手机访问走 TCP 入站，Windows 防火墙默认拦截。首次监听时系统通常会弹窗
//! 生成一条「按程序放行」的规则；弹窗被取消、程序换了路径、或网络类型变化时，
//! 规则就没了，表现是手机一直连不上。这里的职责：
//!
//! 1. 告诉界面当前版本有没有放行（端口规则或系统按程序规则都算）；
//! 2. 需要时用管理员权限补一条**只开一个端口、只允许本地子网**的窄规则；
//! 3. 清理指向旧版本 / 已删除程序的历史规则（Windows 不会自动删）。
//!
//! 检测只读、不需要提权；新增/删除规则需要 UAC。

use serde::Serialize;

/// 我们自己创建的规则名前缀，后面接端口；清理时按 `PointHub*` 识别。
const RULE_PREFIX: &str = "PointHub LAN";

#[derive(Debug, Clone, Serialize)]
pub struct FirewallStatus {
    pub supported: bool,
    pub port: u16,
    pub rule_name: String,
    /// allowed / missing / unknown / unsupported
    pub state: String,
    /// 给人看的说明
    pub detail: String,
    /// 指向旧版本或已不存在程序的残留规则条数
    pub stale_rules: u32,
    /// 当前运行的 exe 是否已被 Windows 按程序放行
    pub current_exe_allowed: bool,
}

pub fn rule_name(port: u16) -> String {
    format!("{RULE_PREFIX} {port}")
}

#[cfg(windows)]
pub use platform::{allow, cleanup_stale, purge_all, status};

#[cfg(not(windows))]
pub use other::{allow, cleanup_stale, purge_all, status};

#[cfg(not(windows))]
mod other {
    use super::*;

    pub fn status(port: u16) -> FirewallStatus {
        FirewallStatus {
            supported: false,
            port,
            rule_name: rule_name(port),
            state: "unsupported".into(),
            detail: "只有 Windows 需要配置防火墙".into(),
            stale_rules: 0,
            current_exe_allowed: false,
        }
    }

    pub fn allow(_port: u16) -> Result<FirewallStatus, String> {
        Err("只有 Windows 需要放行防火墙".into())
    }

    pub fn cleanup_stale(_port: u16) -> Result<FirewallStatus, String> {
        Err("只有 Windows 需要清理防火墙规则".into())
    }

    pub fn purge_all(_quiet: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// 发布版没有控制台；子进程也不该闪黑框。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(serde::Deserialize)]
    struct RawStatus {
        own: bool,
        current: bool,
        stale: u32,
    }

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
            return Err("已取消：放行或清理防火墙需要管理员权限".into());
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

    fn status_script(port: u16) -> String {
        let name = ps_quote(&rule_name(port));
        let exe = ps_quote(&current_exe());
        format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8\n\
             $exe='{exe}'\n\
             $own='{name}'\n\
             $rules=@(Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue)\n\
             $ownOk=$false; $curOk=$false; $stale=0\n\
             foreach($r in $rules){{\n\
               $enabled = ($r.Enabled -eq 'True') -and ($r.Direction -eq 'Inbound') -and ($r.Action -eq 'Allow')\n\
               if($r.DisplayName -eq $own){{ if($enabled){{$ownOk=$true}}else{{$stale++}}; continue }}\n\
               $app=$r | Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue\n\
               $prog=$app.Program\n\
               if($enabled -and $prog -and ($prog -ieq $exe)){{ $curOk=$true; continue }}\n\
               $stale++\n\
             }}\n\
             [pscustomobject]@{{own=$ownOk;current=$curOk;stale=$stale}} | ConvertTo-Json -Compress"
        )
    }

    fn parse_status(raw: &str) -> Result<RawStatus, String> {
        serde_json::from_str(raw.trim()).map_err(|e| format!("无法解析防火墙检测结果：{e}"))
    }

    pub fn status(port: u16) -> FirewallStatus {
        let raw = match run_powershell(&status_script(port)) {
            Ok(out) => out,
            Err(e) => {
                return FirewallStatus {
                    supported: true,
                    port,
                    rule_name: rule_name(port),
                    state: "unknown".into(),
                    detail: e,
                    stale_rules: 0,
                    current_exe_allowed: false,
                }
            }
        };
        let parsed = match parse_status(&raw) {
            Ok(v) => v,
            Err(e) => {
                return FirewallStatus {
                    supported: true,
                    port,
                    rule_name: rule_name(port),
                    state: "unknown".into(),
                    detail: e,
                    stale_rules: 0,
                    current_exe_allowed: false,
                }
            }
        };

        let allowed = parsed.own || parsed.current;
        let detail = if parsed.own {
            "已放行：仅 TCP 端口，允许本地子网访问".to_string()
        } else if parsed.current {
            "Windows 已按程序放行当前版本".to_string()
        } else {
            "未找到放行规则，手机可能连不上".to_string()
        };
        FirewallStatus {
            supported: true,
            port,
            rule_name: rule_name(port),
            state: if allowed { "allowed" } else { "missing" }.into(),
            detail,
            stale_rules: parsed.stale,
            current_exe_allowed: parsed.current,
        }
    }

    fn allow_script(port: u16) -> String {
        let name = ps_quote(&rule_name(port));
        format!(
            "$ProgressPreference='SilentlyContinue'\n\
             netsh advfirewall firewall delete rule name=\"{name}\" | Out-Null\n\
             netsh advfirewall firewall add rule name=\"{name}\" dir=in action=allow \
             protocol=TCP localport={port} profile=domain,private,public remoteip=localsubnet enable=yes | Out-Null\n\
             exit $LASTEXITCODE"
        )
    }

    fn cleanup_script(port: u16) -> String {
        let own = ps_quote(&rule_name(port));
        let exe = ps_quote(&current_exe());
        format!(
            "$exe='{exe}'\n\
             $own='{own}'\n\
             Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue | ForEach-Object {{\n\
               $enabled = ($_.Enabled -eq 'True') -and ($_.Direction -eq 'Inbound') -and ($_.Action -eq 'Allow')\n\
               if($_.DisplayName -eq $own -and $enabled){{ return }}\n\
               $app=$_ | Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue\n\
               $prog=$app.Program\n\
               if($enabled -and $prog -and ($prog -ieq $exe)){{ return }}\n\
               Remove-NetFirewallRule -Name $_.Name -ErrorAction SilentlyContinue\n\
             }}\n\
             exit 0"
        )
    }

    fn purge_script() -> &'static str {
        "Get-NetFirewallRule -DisplayName 'PointHub*' -ErrorAction SilentlyContinue | \
         Remove-NetFirewallRule -ErrorAction SilentlyContinue\n\
         exit 0"
    }

    pub fn allow(port: u16) -> Result<FirewallStatus, String> {
        run_elevated(&allow_script(port))?;
        Ok(status(port))
    }

    pub fn cleanup_stale(port: u16) -> Result<FirewallStatus, String> {
        run_elevated(&cleanup_script(port))?;
        Ok(status(port))
    }

    /// 卸载时删除所有 PointHub 规则。
    ///
    /// 静默卸载没有交互界面，无法弹 UAC，只能跳过并在卸载日志里留痕。
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
        fn rule_name_contains_port() {
            assert_eq!(rule_name(47821), "PointHub LAN 47821");
            assert_eq!(rule_name(1234), "PointHub LAN 1234");
        }

        #[test]
        fn parses_status_json() {
            let raw = r#"{"own":false,"current":true,"stale":14}"#;
            let v = parse_status(raw).unwrap();
            assert!(!v.own);
            assert!(v.current);
            assert_eq!(v.stale, 14);
        }

        #[test]
        fn allow_script_is_narrow() {
            let s = allow_script(47821);
            assert!(s.contains("localport=47821"), "必须只开当前端口");
            assert!(s.contains("remoteip=localsubnet"), "必须限制为本地子网");
            assert!(s.contains("dir=in"), "只放行入站");
            assert!(s.contains("PointHub LAN 47821"));
        }

        #[test]
        fn cleanup_keeps_current_exe_and_own_rule() {
            let s = cleanup_script(47821);
            assert!(s.contains("-ieq $exe"), "清理时必须保留当前版本");
            assert!(s.contains("-eq $own"), "清理时必须保留本次创建的端口规则");
            assert!(s.contains("PointHub*"));
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
