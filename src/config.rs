use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct RegraConformidade {
    pub id_regra: String,
    pub tipo_arquivo: String,
    pub pasta_esperada: String,
    pub nome_arquivo_base: String,
    pub obrigatorio: bool,
    #[serde(default)]
    pub aba_planilha: String, 
    #[serde(default)]
    pub apenas_existencia: bool,
    #[serde(default)]
    pub prompt_ia: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ConfiguracaoGlobal {
    pub endpoint_slm: String,
    pub modelo_slm: String,
    pub regras_conformidade: Vec<RegraConformidade>,
}