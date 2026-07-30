use super::{GitHubIdentity, GitHubIdentityProvider};
use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;
const PROFILE_FILE: &str = "profile.json";

/// GitHub `/user` 返回的公开身份字段。
#[derive(Deserialize)]
struct RemoteGitHubIdentity {
    id: u64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

/// 持久化的公开身份资料，不包含任何 GitHub 凭据。
#[derive(Deserialize, Serialize)]
struct CachedGitHubIdentity {
    id: u64,
    login: String,
    name: Option<String>,
    avatar_file: Option<String>,
}

/// 从 GitHub 获取公开身份，并在应用数据目录维护本地头像缓存。
pub struct FileGitHubIdentityProvider {
    api_base_url: String,
    cache_directory: PathBuf,
    client: reqwest::Client,
}

impl FileGitHubIdentityProvider {
    /// 使用显式 API 地址和身份缓存目录构建 provider。
    pub fn new(api_base_url: impl Into<String>, cache_directory: PathBuf) -> Self {
        Self {
            api_base_url: api_base_url.into().trim_end_matches('/').to_owned(),
            cache_directory,
            client: reqwest::Client::new(),
        }
    }

    /// 将缓存线缆数据转换成 WebView 可见的公开身份。
    fn to_identity(&self, cached: CachedGitHubIdentity) -> GitHubIdentity {
        let avatar_path = cached.avatar_file.and_then(|file| {
            let path = self.cache_directory.join(file);
            path.is_file().then(|| path.to_string_lossy().into_owned())
        });
        GitHubIdentity {
            id: cached.id,
            login: cached.login,
            name: cached.name,
            avatar_path,
        }
    }

    /// 原子写入 JSON，避免中断刷新留下半份身份资料。
    fn save_profile(&self, profile: &CachedGitHubIdentity) -> Result<(), String> {
        std::fs::create_dir_all(&self.cache_directory)
            .map_err(|error| format!("无法创建 GitHub 身份缓存：{error}"))?;
        let temporary = self.cache_directory.join("profile.json.tmp");
        let destination = self.cache_directory.join(PROFILE_FILE);
        let value = serde_json::to_vec(profile)
            .map_err(|error| format!("无法编码 GitHub 身份资料：{error}"))?;
        std::fs::write(&temporary, value)
            .map_err(|error| format!("无法写入 GitHub 身份缓存：{error}"))?;
        std::fs::rename(&temporary, &destination)
            .map_err(|error| format!("无法更新 GitHub 身份缓存：{error}"))
    }

    /// 下载受限大小且格式受支持的头像，成功后返回本地文件名。
    async fn cache_avatar(&self, url: Option<&str>) -> Result<Option<String>, String> {
        let Some(url) = url else {
            return Ok(None);
        };
        if !url.starts_with("https://") && !cfg!(any(test, feature = "wdio")) {
            return Err("GitHub avatar 地址必须使用 HTTPS。".to_owned());
        }
        let response = self
            .client
            .get(url)
            .header(USER_AGENT, "Narrant")
            .send()
            .await
            .map_err(|_| "GitHub avatar 暂时不可用。".to_owned())?
            .error_for_status()
            .map_err(|_| "GitHub avatar 暂时不可用。".to_owned())?;
        if response.content_length().unwrap_or_default() > MAX_AVATAR_BYTES as u64 {
            return Err("GitHub avatar 超过大小限制。".to_owned());
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "GitHub avatar 暂时不可用。".to_owned())?;
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err("GitHub avatar 超过大小限制。".to_owned());
        }
        let extension = supported_image_extension(&content_type, &bytes)
            .ok_or_else(|| "GitHub avatar 格式不受支持。".to_owned())?;
        std::fs::create_dir_all(&self.cache_directory)
            .map_err(|error| format!("无法创建 GitHub 身份缓存：{error}"))?;
        let file_name = format!("avatar.{extension}");
        let temporary = self.cache_directory.join("avatar.tmp");
        std::fs::write(&temporary, &bytes)
            .map_err(|error| format!("无法写入 GitHub avatar：{error}"))?;
        std::fs::rename(&temporary, self.cache_directory.join(&file_name))
            .map_err(|error| format!("无法更新 GitHub avatar：{error}"))?;
        remove_other_avatars(&self.cache_directory, &file_name);
        Ok(Some(file_name))
    }
}

#[async_trait]
impl GitHubIdentityProvider for FileGitHubIdentityProvider {
    /// 从磁盘读取上次成功获取的公开身份。
    async fn cached(&self) -> Result<Option<GitHubIdentity>, String> {
        let path = self.cache_directory.join(PROFILE_FILE);
        let value = match std::fs::read(path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("无法读取 GitHub 身份缓存：{error}")),
        };
        let cached: CachedGitHubIdentity = serde_json::from_slice(&value)
            .map_err(|error| format!("GitHub 身份缓存已损坏：{error}"))?;
        Ok(Some(self.to_identity(cached)))
    }

    /// 使用当前 user access token 刷新 `/user` 资料和本地头像。
    async fn refresh(&self, access_token: &SecretString) -> Result<GitHubIdentity, String> {
        let previous = self.cached().await.ok().flatten();
        let remote: RemoteGitHubIdentity = self
            .client
            .get(format!("{}/user", self.api_base_url))
            .header(USER_AGENT, "Narrant")
            .header(ACCEPT, "application/vnd.github+json")
            .header(
                AUTHORIZATION,
                format!("Bearer {}", access_token.expose_secret()),
            )
            .send()
            .await
            .map_err(|_| "GitHub 身份资料暂时不可用。".to_owned())?
            .error_for_status()
            .map_err(|_| "GitHub 身份资料暂时不可用。".to_owned())?
            .json()
            .await
            .map_err(|_| "GitHub 身份资料响应无效。".to_owned())?;
        let avatar_file = match self.cache_avatar(remote.avatar_url.as_deref()).await {
            Ok(file) => file,
            Err(_)
                if previous
                    .as_ref()
                    .is_some_and(|identity| identity.id == remote.id) =>
            {
                previous
                    .and_then(|identity| identity.avatar_path)
                    .and_then(|path| {
                        Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
            }
            Err(_) => None,
        };
        let cached = CachedGitHubIdentity {
            id: remote.id,
            login: remote.login,
            name: remote.name.filter(|name| !name.trim().is_empty()),
            avatar_file,
        };
        self.save_profile(&cached)?;
        Ok(self.to_identity(cached))
    }

    /// 删除公开身份资料和头像缓存。
    fn clear(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.cache_directory) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("无法清除 GitHub 身份缓存：{error}")),
        }
    }
}

/// 依据响应类型和文件签名选择安全的本地扩展名。
fn supported_image_extension(content_type: &str, bytes: &[u8]) -> Option<&'static str> {
    if content_type.starts_with("image/png") && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if content_type.starts_with("image/jpeg") && bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if content_type.starts_with("image/gif")
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
    {
        Some("gif")
    } else if content_type.starts_with("image/webp")
        && bytes.starts_with(b"RIFF")
        && bytes.get(8..12) == Some(b"WEBP")
    {
        Some("webp")
    } else {
        None
    }
}

/// 删除上次刷新遗留的其他格式头像文件。
fn remove_other_avatars(directory: &Path, retained_file: &str) {
    for extension in ["png", "jpg", "gif", "webp"] {
        let file_name = format!("avatar.{extension}");
        if file_name != retained_file {
            let _ = std::fs::remove_file(directory.join(file_name));
        }
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
