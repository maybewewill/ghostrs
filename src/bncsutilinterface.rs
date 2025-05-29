use crate::util::*;
use crate::bncsutil::*;
use crate::logger::*;
use std::path::Path;

const BNCSUTIL_PLATFORM_X86: u32 = 1;
#[derive(Debug, Clone)]
pub struct BNCSUtilInterface {
    m_NLS: usize,
    m_EXEVersion: ByteArray,
    m_EXEVersionHash: ByteArray,
    m_EXEInfo: String,
    m_KeyInfoROC: ByteArray,
    m_KeyInfoTFT: ByteArray,
    m_ClientKey: ByteArray,
    m_M1: ByteArray,
    m_PvPGNPasswordHash: ByteArray,
}

impl BNCSUtilInterface {
    pub fn new(username: &str, password: &str) -> Self {
        let bncs = BncsUtil::new().unwrap();
        let nls = bncs.nls_init(username, password).unwrap();
        BNCSUtilInterface {
            m_NLS: nls,
            m_EXEVersion: ByteArray::new(),
            m_EXEVersionHash: ByteArray::new(),
            m_EXEInfo: String::new(),
            m_KeyInfoROC: ByteArray::new(),
            m_KeyInfoTFT: ByteArray::new(),
            m_ClientKey: ByteArray::new(),
            m_M1: ByteArray::new(),
            m_PvPGNPasswordHash: ByteArray::new(),
        }
    }

    pub fn get_exe_version(&self) -> &ByteArray {
        &self.m_EXEVersion
    }
    pub fn get_exe_version_hash(&self) -> &ByteArray {
        &self.m_EXEVersionHash
    }
    pub fn get_exe_info(&self) -> &String {
        &self.m_EXEInfo
    }
    pub fn get_key_info_roc(&self) -> &ByteArray {
        &self.m_KeyInfoROC
    }
    pub fn get_key_info_tft(&self) -> &ByteArray {
        &self.m_KeyInfoTFT
    }
    pub fn get_client_key(&self) -> &ByteArray {
        &self.m_ClientKey
    }
    pub fn get_m1(&self) -> &ByteArray {
        &self.m_M1
    }
    pub fn get_pvpgn_password_hash(&self) -> &ByteArray {
        &self.m_PvPGNPasswordHash
    }

    pub fn set_exe_version(&mut self, version: ByteArray) {
        self.m_EXEVersion = version;
    }
    pub fn set_exe_version_hash(&mut self, version_hash: ByteArray) {
        self.m_EXEVersionHash = version_hash;
    }

    pub fn reset(&mut self, username: &str, password: &str) {
        let bncs = BncsUtil::new().unwrap();
        self.m_NLS = bncs.nls_init(username, password).unwrap();
    }

    pub fn HELP_SID_AUTH_CHECK(
        &mut self,
        tft: bool,
        war3_version: u32,
        war3_path: &str,
        key_roc: &str,
        key_tft: &str,
        value_string_formula: &str,
        mpq_file_name: &str,
        client_token: ByteArray,
        server_token: ByteArray,
    ) -> bool {
        let mut file_war3_exe = format!("{}/Warcraft III.exe", war3_path);
        if !Path::new(&file_war3_exe).exists() {
            file_war3_exe = format!("{}/warcraft.exe", war3_path);
        }

        let mut missing_file = false;

        if !Path::new(&file_war3_exe).exists() {
            log_error(&format!(
                "[BNCSUI] unable to open [{}]",
                file_war3_exe
            ));
            missing_file = true;
        }

        let mut file_storm_dll = String::new();
        let mut file_game_dll = String::new();

        if war3_version <= 28 {
            file_storm_dll = format!("{}/Storm.dll", war3_path);
            if !Path::new(&file_storm_dll).exists() {
                file_storm_dll = format!("{}/storm.dll", war3_path);
            }
            file_game_dll = format!("{}/game.dll", war3_path);

            if !Path::new(&file_storm_dll).exists() {
                log_error(&format!(
                    "[BNCSUI] unable to open [{}]",
                    file_storm_dll
                ));
                missing_file = true;
            }
            if !Path::new(&file_game_dll).exists() {
                log_error(&format!(
                    "[BNCSUI] unable to open [{}]",
                    file_game_dll
                ));
                missing_file = true;
            }
        }

        if missing_file {
            return false;
        }

        let bncs = BncsUtil::new().unwrap();
        let (exe_info, version) = bncs.get_exe_info(&file_war3_exe, BNCSUTIL_PLATFORM_X86).unwrap();
        self.m_EXEInfo = exe_info;
        self.m_EXEVersion = create_byte_array_from_u32(version, false);

        let mpq_number = bncs.extract_mpq_number(mpq_file_name).unwrap();
        let exe_version_hash = if war3_version <= 28 {
            bncs.check_revision_flat(
                value_string_formula,
                &file_war3_exe,
                &file_storm_dll,
                &file_game_dll,
                mpq_number,
            )
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "Unsupported war3_version"))
        }.unwrap();

        self.m_EXEVersionHash = create_byte_array_from_u32(exe_version_hash, false);
        self.m_KeyInfoROC = self.create_key_info(
            key_roc,
            byte_array_to_u32(&client_token, false, 0),
            byte_array_to_u32(&server_token, false, 0),
        );

        if tft {
            self.m_KeyInfoTFT = self.create_key_info(
                key_tft,
                byte_array_to_u32(&client_token, false, 0),
                byte_array_to_u32(&server_token, false, 0),
            );
        }

        if self.m_KeyInfoROC.len() == 36 && (!tft || self.m_KeyInfoTFT.len() == 36) {
            true
        } else {
            if self.m_KeyInfoROC.len() != 36 {
                log_error("[BNCSUI] unable to create ROC key info - invalid ROC key");
            }
            if tft && self.m_KeyInfoTFT.len() != 36 {
                log_error("[BNCSUI] unable to create TFT key info - invalid TFT key");
            }
            false
        }
    }

    pub fn HELP_SID_AUTH_ACCOUNTLOGON(&mut self) -> bool {
        let bncs = BncsUtil::new().unwrap();
        self.m_ClientKey = bncs.nls_get_a(self.m_NLS).unwrap();
        true
    }

    pub fn HELP_SID_AUTH_ACCOUNTLOGONPROOF(&mut self, salt: ByteArray, server_key: ByteArray) -> bool {
        let bncs = BncsUtil::new().unwrap();
        self.m_M1 = bncs.nls_get_m1(self.m_NLS, &server_key, &salt).unwrap();
        true
    }

    pub fn HELP_PvPGNPasswordHash(&mut self, user_password: &str) -> bool {
        let bncs = BncsUtil::new().unwrap();
        self.m_PvPGNPasswordHash = bncs.hash_password(user_password).unwrap();
        true
    }

    fn create_key_info(&self, key: &str, client_token: u32, server_token: u32) -> ByteArray {
        let mut key_info = ByteArray::new();
        let bncs = BncsUtil::new().unwrap();
        let cd_key = bncs.kd_quick(key, client_token, server_token).unwrap();

        if !cd_key.hash.is_empty() {
            append_byte_array_from_u32(&mut key_info, key.len() as u32, false);
            append_byte_array_from_u32(&mut key_info, cd_key.product, false);
            append_byte_array_from_u32(&mut key_info, cd_key.public_value, false);
            key_info.extend_from_slice(&[0, 0, 0, 0]);
            key_info.extend_from_slice(&cd_key.hash);
        }

        key_info
    }
}

