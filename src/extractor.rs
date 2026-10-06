use std::fs;
use std::io::Read;
use std::path::Path;
use calamine::{open_workbook, DataType, Reader, Xlsx};

/// Extrai texto limpo de uma aba específica de um arquivo Excel (.xlsx)
pub fn extrair_excel(caminho: &Path, abas: &str) -> Result<String, String> {
    let mut planilha: Xlsx<_> = open_workbook(caminho)
        .map_err(|e| format!("Erro ao abrir Excel: {}", e))?;
    
    let mut dados_extraidos = String::new();
    
    // Divide os nomes das abas pela vírgula e remove os espaços
    let lista_abas: Vec<&str> = abas.split(',').map(|s| s.trim()).collect();

    for aba in lista_abas {
        if let Some(Ok(faixa)) = planilha.worksheet_range(aba) {
            dados_extraidos.push_str(&format!("\n--- CONTEÚDO DA ABA: {} ---\n", aba));
            for linha in faixa.rows() {
                let linha_texto: Vec<String> = linha.iter().map(|celula| match celula {
                    DataType::String(s) => s.to_string(),
                    DataType::Float(f) => f.to_string(),
                    DataType::Int(i) => i.to_string(),
                    DataType::Bool(b) => b.to_string(),
                    _ => String::from(" "),
                }).collect();
                dados_extraidos.push_str(&linha_texto.join(" | "));
                dados_extraidos.push('\n');
            }
        } else {
            dados_extraidos.push_str(&format!("\n[Aviso: Aba '{}' não encontrada ou vazia]\n", aba));
        }
    }
    Ok(dados_extraidos)
}

/// Abre o ZIP oculto do Word (.docx) e extrai o texto bruto contido no XML
pub fn extrair_word(caminho: &Path) -> Result<String, String> {
    let arquivo = fs::File::open(caminho)
        .map_err(|e| format!("Erro ao abrir arquivo DOCX: {}", e))?;
    
    let mut archive = zip::ZipArchive::new(arquivo)
        .map_err(|e| format!("Erro ao ler estrutura ZIP do DOCX: {}", e))?;
    
    let mut xml = archive.by_name("word/document.xml")
        .map_err(|e| format!("Arquivo document.xml interno ausente: {}", e))?;
        
    let mut conteudo = String::new();
    xml.read_to_string(&mut conteudo).unwrap();

    let mut texto_final = String::new();
    for pedaco in conteudo.split("<w:t").skip(1) {
        if let Some(fim_tag) = pedaco.find('>') {
            if let Some(inicio_fechamento) = pedaco.find("</w:t>") {
                texto_final.push_str(&pedaco[fim_tag + 1..inicio_fechamento]);
                texto_final.push_str(" ");
            }
        }
    }
    Ok(texto_final)
}