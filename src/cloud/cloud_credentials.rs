use crate::{ffi, ffi_util::CStrLike, Error};

/// AWS access type for cloud credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AwsAccessType {
    Undefined = ffi::rocksdb_cloud_aws_access_undefined as i32,
    Simple = ffi::rocksdb_cloud_aws_access_simple as i32,
    Instance = ffi::rocksdb_cloud_aws_access_instance as i32,
    TaskRole = ffi::rocksdb_cloud_aws_access_task_role as i32,
    Environment = ffi::rocksdb_cloud_aws_access_environment as i32,
    Config = ffi::rocksdb_cloud_aws_access_config as i32,
    Anonymous = ffi::rocksdb_cloud_aws_access_anonymous as i32,
}

/// Cloud credentials used to authenticate with cloud storage providers.
pub struct CloudCredentials {
    pub(crate) inner: *mut ffi::rocksdb_cloud_credentials_t,
}

unsafe impl Send for CloudCredentials {}
unsafe impl Sync for CloudCredentials {}

impl Drop for CloudCredentials {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_credentials_destroy(self.inner);
        }
    }
}

impl Default for CloudCredentials {
    fn default() -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_credentials_create() };
        assert!(!inner.is_null(), "Could not create CloudCredentials");
        Self { inner }
    }
}

impl CloudCredentials {
    /// Initialize credentials with a simple access key and secret key pair.
    pub fn initialize_simple(&mut self, access_key_id: impl CStrLike, secret_key: impl CStrLike) {
        let access_key_id = access_key_id.into_c_string().unwrap();
        let secret_key = secret_key.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_credentials_initialize_simple(
                self.inner,
                access_key_id.as_ptr(),
                secret_key.as_ptr(),
            );
        }
    }

    /// Initialize credentials from a config file.
    pub fn initialize_config(&mut self, config_file: impl CStrLike) {
        let config_file = config_file.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_credentials_initialize_config(self.inner, config_file.as_ptr());
        }
    }

    /// Set the AWS access type.
    pub fn set_type(&mut self, access_type: AwsAccessType) {
        unsafe {
            ffi::rocksdb_cloud_credentials_set_type(self.inner, access_type as libc::c_int);
        }
    }

    /// Get the AWS access type.
    pub fn get_type(&self) -> AwsAccessType {
        let t = unsafe { ffi::rocksdb_cloud_credentials_get_type(self.inner) };
        match t as u32 {
            ffi::rocksdb_cloud_aws_access_simple => AwsAccessType::Simple,
            ffi::rocksdb_cloud_aws_access_instance => AwsAccessType::Instance,
            ffi::rocksdb_cloud_aws_access_task_role => AwsAccessType::TaskRole,
            ffi::rocksdb_cloud_aws_access_environment => AwsAccessType::Environment,
            ffi::rocksdb_cloud_aws_access_config => AwsAccessType::Config,
            ffi::rocksdb_cloud_aws_access_anonymous => AwsAccessType::Anonymous,
            _ => AwsAccessType::Undefined,
        }
    }

    /// Check whether the credentials are valid.
    pub fn has_valid(&self) -> Result<bool, Error> {
        unsafe {
            let mut err = std::ptr::null_mut();
            let result = ffi::rocksdb_cloud_credentials_has_valid(self.inner, &mut err);
            if !err.is_null() {
                let msg = crate::ffi_util::from_cstr_and_free(err);
                Err(Error::new(msg))
            } else {
                Ok(result != 0)
            }
        }
    }
}
