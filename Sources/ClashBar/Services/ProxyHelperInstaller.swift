import Darwin
import Foundation
import ProxyHelperShared
import ServiceManagement

enum ProxyHelperInstaller {
    static func installedHelperIsCurrent() -> Bool {
        let recorded = try? String(contentsOfFile: ProxyHelperConstants.installedVersionPath, encoding: .utf8)
        return FileManager.default.isExecutableFile(atPath: ProxyHelperConstants.installedHelperPath)
            && FileManager.default.fileExists(atPath: ProxyHelperConstants.installedPlistPath)
            && recorded?.trimmingCharacters(in: .whitespacesAndNewlines) == String(ProxyHelperConstants.helperVersion)
            && !self.hasQuarantine(atPath: ProxyHelperConstants.installedHelperPath)
            && !self.hasQuarantine(atPath: ProxyHelperConstants.installedPlistPath)
    }

    static func hasQuarantine(atPath path: String) -> Bool {
        getxattr(path, "com.apple.quarantine", nil, 0, 0, 0) >= 0
    }

    static func installBundledHelper() async throws {
        let bundleURL = Bundle.main.bundleURL
        let command = self.installShellCommand(
            sourceBinary: bundleURL.appendingPathComponent(ProxyHelperConstants.helperBundleProgram).path,
            sourcePlist: bundleURL.appendingPathComponent(ProxyHelperConstants.helperBundlePlist).path)

        try? await SMAppService.daemon(plistName: ProxyHelperConstants.daemonPlistName).unregister()
        try await Task.detached(priority: .userInitiated) {
            try AdministratorShell.run(
                command,
                cancelled: SystemProxyServiceError.helperAuthorizationCancelled,
                failed: SystemProxyServiceError.helperInstallFailed)
        }.value
    }

    private static func installShellCommand(sourceBinary: String, sourcePlist: String) -> String {
        let binary = AdministratorShell.shellQuoted(sourceBinary)
        let plist = AdministratorShell.shellQuoted(sourcePlist)
        let destBinary = AdministratorShell.shellQuoted(ProxyHelperConstants.installedHelperPath)
        let destPlist = AdministratorShell.shellQuoted(ProxyHelperConstants.installedPlistPath)
        let versionFile = AdministratorShell.shellQuoted(ProxyHelperConstants.installedVersionPath)
        let label = ProxyHelperConstants.machServiceName

        return """
        set -e
        /bin/mkdir -p /Library/PrivilegedHelperTools /Library/LaunchDaemons
        /bin/rm -f \(versionFile)
        /bin/launchctl bootout system/\(label) >/dev/null 2>&1 || true
        /bin/rm -f \(destBinary)
        /bin/cp -X \(binary) \(destBinary)
        /usr/sbin/chown root:wheel \(destBinary)
        /bin/chmod 755 \(destBinary)
        /usr/bin/xattr -c \(destBinary) 2>/dev/null || true
        /usr/bin/xattr -d com.apple.quarantine \(destBinary) 2>/dev/null || true
        /bin/rm -f \(destPlist)
        /bin/cp -X \(plist) \(destPlist)
        /usr/sbin/chown root:wheel \(destPlist)
        /bin/chmod 644 \(destPlist)
        /usr/bin/xattr -c \(destPlist) 2>/dev/null || true
        /usr/bin/xattr -d com.apple.quarantine \(destPlist) 2>/dev/null || true
        if /usr/bin/xattr -p com.apple.quarantine \(destBinary) >/dev/null 2>&1; then
            echo "Failed to remove quarantine attribute from helper binary." >&2
            exit 1
        fi
        if /usr/bin/xattr -p com.apple.quarantine \(destPlist) >/dev/null 2>&1; then
            echo "Failed to remove quarantine attribute from helper plist." >&2
            exit 1
        fi
        /bin/launchctl enable system/\(label) >/dev/null 2>&1 || true
        if ! /bin/launchctl bootstrap system \(destPlist) 2>/dev/null; then
            /bin/sleep 0.1
            /bin/launchctl bootout system/\(label) >/dev/null 2>&1 || true
            /bin/launchctl bootstrap system \(destPlist)
        fi
        /bin/launchctl kickstart -k system/\(label)
        echo \(ProxyHelperConstants.helperVersion) > \(versionFile)
        """
    }
}
