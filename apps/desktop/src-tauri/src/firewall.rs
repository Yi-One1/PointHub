//! Windows 防火墙：安装时自动放行手机访问，卸载时自动清理。
//!
//! 手机访问走 TCP 入站，Windows 防火墙默认拦截。防火墙规则只有管理员能改，
//! 所以无法在运行期完全静默处理；这里把唯一一次授权放到安装完成时：
//!
//! - 安装器调用 `pointhub.exe --firewall-install`，检查当前安装目录是否已放行；
//!   已放行则直接返回（原地升级不会重复弹 UAC），否则以本程序身份请求提权
//!   （UAC 里显示 PointHub，而不是 powershell.exe），再写入一条按程序放行的规则；
//! - 规则只对本地子网生效，程序升级、改端口都不受影响；
//! - 卸载时由 `purge_all` 提权删除全部 PointHub 规则。
//!
//! 便携版没有安装器，由 Windows 首次监听时的原生弹窗处理；应用内不再提示。

#[cfg(windows)]
const RULE_NAME: &str = "PointHub LAN";

/// 安装完成时调用：确保当前安装路径已有放行规则。
///
/// `quiet` 用于静默安装：没有交互界面，无法弹 UAC，只能跳过。
/// `elevated` 表示当前进程已经拿到管理员权限（由本程序重新拉起），直接执行。
#[cfg(windows)]
pub fn install(quiet: bool, elevated: bool) -> Result<(), String> {
    platform::install(quiet, elevated)
}

/// 卸载时删除全部 PointHub 防火墙规则。
#[cfg(windows)]
pub fn purge_all(quiet: bool, elevated: bool) -> Result<(), String> {
    platform::purge_all(quiet, elevated)
}

#[cfg(not(windows))]
pub fn install(_quiet: bool, _elevated: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn purge_all(_quiet: bool, _elevated: bool) -> Result<(), String> {
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

    /// 以管理员身份重新启动本程序（保留原参数，追加 `--elevated`）。
    ///
    /// 提权的对象是 `pointhub.exe` 自己，所以 UAC 弹窗显示应用名称和图标；
    /// 若直接提权 powershell.exe，用户会看到「Windows PowerShell」而困惑。
    fn elevate_self(action: &str) -> Result<(), String> {
        let mut args: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| a != "--elevated")
            .collect();
        args.push("--elevated".into());
        let script = elevated_launch_script(&current_exe(), &args);
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("无法启动提权进程：{e}"))?;
        let code = out.status.code().unwrap_or(-1);
        if code == 0 {
            return Ok(());
        }
        if code == 1223 {
            return Err(format!("已取消：{action}"));
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() {
            format!("操作失败（退出码 {code}）")
        } else {
            format!("操作失败（退出码 {code}）：{err}")
        })
    }

    /// 外层 PowerShell：请求 UAC 拉起本程序，并原样带回退出码。
    fn elevated_launch_script(exe: &str, args: &[String]) -> String {
        let exe = ps_quote(exe);
        let list = args
            .iter()
            .map(|a| format!("'{}'", ps_quote(a)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "try {{ $p = Start-Process -FilePath '{exe}' -ArgumentList {list} \
             -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode }} \
             catch {{ exit 1223 }}"
        )
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

    pub fn install(quiet: bool, elevated: bool) -> Result<(), String> {
        if has_rule()? {
            return Ok(());
        }
        if quiet {
            return Err("静默安装无法提权，跳过防火墙规则安装".into());
        }
        if elevated {
            run_powershell(&install_script())?;
            return Ok(());
        }
        elevate_self("修改防火墙需要管理员权限")
    }

    pub fn purge_all(quiet: bool, elevated: bool) -> Result<(), String> {
        let count = count_rules()?;
        if count == 0 {
            return Ok(());
        }
        if quiet {
            return Err(format!(
                "静默卸载无法提权，跳过 {count} 条防火墙规则清理"
            ));
        }
        if elevated {
            run_powershell(purge_script())?;
            return Ok(());
        }
        elevate_self("清理防火墙规则需要管理员权限")
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
        fn elevated_launch_targets_our_own_exe() {
            let script = elevated_launch_script(
                r"C:\Apps\PointHub\pointhub.exe",
                &["--firewall-install".to_string(), "--elevated".to_string()],
            );
            assert!(script.contains("-Verb RunAs"), "必须请求提权");
            assert!(
                script.contains(r"'C:\Apps\PointHub\pointhub.exe'"),
                "提权对象应当是应用自己，而不是 powershell.exe"
            );
            assert!(
                script.contains("'--firewall-install','--elevated'"),
                "重新拉起时要保留原参数并标记已提权"
            );
            assert!(
                !script.contains("powershell.exe' -Verb"),
                "不能再让 UAC 显示 Windows PowerShell"
            );
        }

        #[test]
        fn ps_quote_doubles_single_quote() {
            assert_eq!(ps_quote("a'b"), "a''b");
        }
    }
}
