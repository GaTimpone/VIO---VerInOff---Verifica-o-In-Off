use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct RegraConformidade {
    pub id_regra: String,
    pub tipo_arquivo: String,
    pub pasta_esperada: String,
    pub nome_arquivo_base: String,
    pub obrigatorio: bool,
    #[serde(default)]
    pub apenas_existencia: Option<bool>,
    #[serde(default)]
    pub aba_planilha: Option<String>,
    #[serde(default)]
    pub prompt_ia: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfiguracaoGlobal {
    pub endpoint_slm: String,
    pub modelo_slm: String,
    #[serde(default)]
    pub pastas_ignoradas_extracao: Option<Vec<String>>,
    pub regras_conformidade: Vec<RegraConformidade>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SettingsLocal {
    pub quiet_mode: bool,
}
