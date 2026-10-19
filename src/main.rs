use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::net::TcpStream;
use std::time::Duration;
use std::thread;
use std::collections::HashSet;
use futures_util::StreamExt;
use std::io::{self, Write};
use rusqlite::Connection;
use regex::Regex;

mod config;
mod extractor;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RegraConformidade {
    id_regra: String,
    tipo_arquivo: String,
    pasta_esperada: String,
    nome_arquivo_base: String,
    obrigatorio: bool,
    apenas_existencia: Option<bool>,
    aba_planilha: Option<String>,
    prompt_ia: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ConfiguracaoGlobal {
    endpoint_slm: String,
    modelo_slm: String,
    pastas_ignoradas_extracao: Option<Vec<String>>,
    regras_conformidade: Vec<RegraConformidade>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SettingsLocal {
    quiet_mode: bool,
}

#[derive(Parser, Debug)]
#[command(version, about = "VIO - Motor de Verificação de Processos Corporativos", long_about = None)]
struct ArgumentosCli {
    #[arg(default_value = ".")]
    caminho_projeto: String,

    #[arg(help = "Arquivo JSON (opcional)")]
    arquivo_config: Option<String>,

    #[arg(long, help = "Exibe o banco de dados em formato bruto")]
    mostrar_banco: bool,

    #[arg(long, help = "Gera a tabela final de relatório puxando do banco de dados")]
    gerar_relatorio: bool,

    #[arg(long)]
    projeto: Option<String>,

    #[arg(long)]
    fase: Option<String>,

    #[arg(long)]
    id_regra: Option<String>,

    #[arg(long)]
    limpar_banco: bool,

    #[arg(long)]
    update: bool,

    #[arg(long, help = "Ativa ou desativa permanentemente o modo silencioso")]
    toggle_quiet: bool,

    #[arg(long, help = "Exporta o relatório gerado para um arquivo CSV")]
    exportar_csv: bool,

    #[arg(long, help = "Caminho específico para salvar o CSV (ex: C:\\Relatorios\\saida.csv)")]
    saida_csv: Option<String>,
}

#[derive(serde::Serialize)]
struct OllamaOptions {
    num_ctx: u32,
    num_predict: u32,
    temperature: f32,
    top_k: u32,
    top_p: f32,
}

#[derive(serde::Serialize)]
struct PayloadOllama {
    model: String,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
}

fn gerenciar_motor_ia(endpoint: &str) -> Option<Child> {
    let endereço_porta = endpoint.replace("http://", "").replace("/api/generate", "").replace("localhost", "127.0.0.1");
    if TcpStream::connect_timeout(&endereço_porta.parse().unwrap(), Duration::from_secs(1)).is_ok() {
        return None;
    }
    if let Ok(child) = Command::new("ollama").arg("serve").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        thread::sleep(Duration::from_secs(5));
        return Some(child);
    }
    None
}

fn buscar_arquivos_estranhos(dir: &Path, arquivos_conhecidos: &HashSet<String>, ignoradas: &Vec<String>, achados: &mut Vec<String>) {
    if let Ok(entradas) = fs::read_dir(dir) {
        for entrada in entradas.flatten() {
            let path = entrada.path();
            let nome_pasta = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if ignoradas.iter().any(|ig| nome_pasta.contains(ig)) { continue; }
            if path.is_dir() {
                buscar_arquivos_estranhos(&path, arquivos_conhecidos, ignoradas, achados);
            } else if path.is_file() {
                let caminho_str = path.to_string_lossy().replace("\\", "/");
                if !arquivos_conhecidos.contains(&caminho_str) { achados.push(caminho_str); }
            }
        }
    }
}

fn truncar(texto: &str, limite: usize) -> String {
    if texto.chars().count() > limite {
        let mut truncado: String = texto.chars().take(limite - 3).collect();
        truncado.push_str("...");
        truncado
    } else { texto.to_string() }
}

#[tokio::main]
async fn main() {
    let args = ArgumentosCli::parse();
    let mut caminho_executavel = std::env::current_exe().unwrap();
    caminho_executavel.pop();

    let caminho_settings = caminho_executavel.join("settings.json");
    let mut settings = if caminho_settings.exists() {
        serde_json::from_str(&fs::read_to_string(&caminho_settings).unwrap()).unwrap_or(SettingsLocal { quiet_mode: false })
    } else {
        SettingsLocal { quiet_mode: false }
    };

    if args.toggle_quiet {
        settings.quiet_mode = !settings.quiet_mode;
        fs::write(&caminho_settings, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        println!("⚙️ Modo Silencioso alterado para: {}", if settings.quiet_mode { "ATIVADO" } else { "DESATIVADO" });
        return;
    }

    let is_quiet = settings.quiet_mode;

    let caminho_banco = caminho_executavel.join("auditorias.db");
    let conexao_db = Connection::open(&caminho_banco).unwrap();
    conexao_db.execute("CREATE TABLE IF NOT EXISTS identidade_projeto (projeto TEXT PRIMARY KEY, resumo_identidade TEXT NOT NULL)", []).ok();
    conexao_db.execute("CREATE TABLE IF NOT EXISTS historico_auditoria (id INTEGER PRIMARY KEY AUTOINCREMENT, projeto TEXT NOT NULL, fase_json TEXT NOT NULL, id_regra TEXT NOT NULL, arquivo TEXT NOT NULL, status TEXT NOT NULL, desvio TEXT DEFAULT '', sugestao TEXT DEFAULT '', data_auditoria DATETIME DEFAULT CURRENT_TIMESTAMP)", []).ok();

    if args.limpar_banco {
        conexao_db.execute("DROP TABLE IF EXISTS historico_auditoria", []).ok();
        conexao_db.execute("DROP TABLE IF EXISTS identidade_projeto", []).ok();
        println!("🗑️ Banco de dados limpo!");
        return;
    }

    // Busca Flexível (Não liga para Maiúsculas e Minúsculas graças ao LIKE)
    if args.mostrar_banco {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(ref p) = args.projeto { sql.push_str(" AND projeto LIKE ?"); params.push(format!("%{}%", p)); }
        if let Some(ref f) = args.fase { sql.push_str(" AND fase_json LIKE ?"); params.push(format!("%{}%", f)); }
        if let Some(ref r) = args.id_regra { sql.push_str(" AND id_regra LIKE ?"); params.push(format!("%{}%", r)); }
        
        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(format!("PROJ: {} | STATUS: {:<12} | REGRA: {}\n -> DESVIO: {}\n -> SUGESTÃO: {}\n", row.get::<_, String>(0)?, row.get::<_, String>(3)?, row.get::<_, String>(2)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
        }).unwrap();
        for audit in auditorias { println!("{}", audit.unwrap()); }
        return;
    }

    // Relatório Flexível
    if args.gerar_relatorio {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if let Some(ref p) = args.projeto { sql.push_str(" AND projeto LIKE ?"); params.push(format!("%{}%", p)); }
        if let Some(ref f) = args.fase { sql.push_str(" AND fase_json LIKE ?"); params.push(format!("%{}%", f)); }

        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?
            ))
        }).unwrap();

        let mut registros = Vec::new();
        for audit in auditorias { registros.push(audit.unwrap()); }

        if registros.is_empty() {
            println!("Nenhum registro encontrado para os filtros informados.");
            return;
        }

        let mut qtd_valido = 0.0;
        let mut qtd_sim = 0.0;

        for reg in &registros {
            let st = reg.3.to_lowercase();
            if st.contains("sim") { qtd_sim += 1.0; qtd_valido += 1.0; }
            else if st.contains("não") || st.contains("nao") || st.contains("parcialmente") {
                if !st.contains("aplica") { qtd_valido += 1.0; }
            }
        }

        let indicador = if qtd_valido > 0.0 { (qtd_sim / qtd_valido) * 100.0 } else { 0.0 };

        println!("\n====================================================================================================");
        println!("                                  RELATÓRIO DE CONFORMIDADE (VIA BANCO)");
        println!("====================================================================================================");
        println!("Aderência : {:.1}% (Indicador do Processo)", indicador);
        println!("----------------------------------------------------------------------------------------------------");
        
        for reg in &registros {
            println!("[{:<12}] {}", reg.3, reg.2);
            let desvio = reg.4.trim();
            let sugestao = reg.5.trim();
            
            if !desvio.is_empty() && desvio.to_lowercase() != "nenhum" && desvio.to_lowercase() != "nenhuma." {
                println!("  ↳ Desvio  : {}", desvio);
            }
            if !sugestao.is_empty() && sugestao.to_lowercase() != "nenhuma" && sugestao.to_lowercase() != "nenhuma." {
                println!("  ↳ Sugestão: {}", sugestao);
            }
            println!("-");
        }
        println!("====================================================================================================\n");

        if args.exportar_csv {
            let caminho_salvar = args.saida_csv.unwrap_or_else(|| "Relatorio_Auditoria_VIO.csv".to_string());
            let mut csv_content = String::from("Projeto;Fase;Status;Pergunta;Desvio;Sugestao\n");
            for reg in &registros {
                csv_content.push_str(&format!("{};{};{};{};{};{}\n",
                    reg.0, reg.1, reg.3,
                    reg.2.replace(";", ","), 
                    reg.4.replace(";", ",").replace("\n", " "), 
                    reg.5.replace(";", ",").replace("\n", " ")
                ));
            }
            fs::write(&caminho_salvar, csv_content).expect("Falha ao salvar CSV");
            let path_absoluto = fs::canonicalize(&caminho_salvar).unwrap_or_else(|_| PathBuf::from(&caminho_salvar));
            println!("💾 Relatório exportado para Excel com sucesso em:\n   {}", path_absoluto.display());
        }
        return;
    }

    let pasta_atual = std::env::current_dir().unwrap();
    let mut pasta_regras = pasta_atual.join("config_rules");
    if !pasta_regras.exists() { pasta_regras = caminho_executavel.join("config_rules"); }

    let jsons_para_rodar = match &args.arquivo_config {
        Some(nome) => vec![nome.clone()],
        None => {
            let mut files = vec![];
            if let Ok(entradas) = fs::read_dir(&pasta_regras) {
                for entrada in entradas.flatten() {
                    if entrada.path().extension().unwrap_or_default() == "json" { files.push(entrada.file_name().to_string_lossy().to_string()); }
                }
            }
            files
        }
    };

    let diretorio_alvo_projeto = Path::new(&args.caminho_projeto);
    let sigla_projeto = diretorio_alvo_projeto.file_name().unwrap_or_default().to_string_lossy().to_string();
    let cliente_http = reqwest::Client::new();
    let mut processo_ia_gerenciado = None;

    for arquivo_json in jsons_para_rodar {
        let caminho_json = pasta_regras.join(&arquivo_json);
        if !caminho_json.exists() { continue; }
        
        let conteudo_json = match fs::read_to_string(&caminho_json) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Proteção contra JSONs quebrados
        let configuracao: ConfiguracaoGlobal = match serde_json::from_str(&conteudo_json) {
            Ok(config) => config,
            Err(erro) => {
                println!("⚠️ Pulando o arquivo '{}': Erro na estrutura do JSON ({}).", arquivo_json, erro);
                continue;
            }
        };
        
        if processo_ia_gerenciado.is_none() { processo_ia_gerenciado = gerenciar_motor_ia(&configuracao.endpoint_slm); }

        let mut arquivos_conhecidos = HashSet::new();
        let mut buffer_banco_dados: Vec<(String, String, String, String, String, String, String)> = Vec::new();
        let mut memoria_do_projeto = String::new();
        let mut identidade_projeto: String = conexao_db.query_row("SELECT resumo_identidade FROM identidade_projeto WHERE projeto = ?1", [&sigla_projeto], |row| row.get(0)).unwrap_or_default();

        let re = Regex::new(r"(?is)(?:Análise|Analise|ANALISAÇÃO).*?:\s*(.*?)\s*Desvio.*?:\s*(.*?)\s*(?:Sugestão|Sugestao|SUGÉSTAO|SUGÉSTIMO).*?:\s*(.*?)\s*Status Qualitativo.*?:\s*[*_]*\s*(Sim|Não|Nao|Parcialmente)").unwrap();

        for regra in configuracao.regras_conformidade {
            let nome_base_resolvido = regra.nome_arquivo_base.replace("{SIGLA}", &sigla_projeto);
            let pasta_alvo = diretorio_alvo_projeto.join(&regra.pasta_esperada);
            let mut caminho_arquivo_alvo = None;

            if pasta_alvo.exists() && pasta_alvo.is_dir() {
                if let Ok(entradas) = fs::read_dir(&pasta_alvo) {
                    for entrada in entradas.flatten() {
                        let caminho = entrada.path();
                        if caminho.is_file() {
                            if let Some(stem) = caminho.file_stem() {
                                if stem.to_string_lossy() == nome_base_resolvido {
                                    caminho_arquivo_alvo = Some(caminho.clone());
                                    arquivos_conhecidos.insert(caminho.to_string_lossy().replace("\\", "/"));
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(caminho_real) = caminho_arquivo_alvo {
                if regra.apenas_existencia.unwrap_or(false) {
                    buffer_banco_dados.push((sigla_projeto.clone(), arquivo_json.clone(), regra.id_regra.clone(), nome_base_resolvido.clone(), "Sim".to_string(), "Nenhum".to_string(), "Nenhuma".to_string()));
                    continue; 
                }

                let resultado_extracao = match regra.tipo_arquivo.as_str() {
                    "excel" => extractor::extrair_excel(&caminho_real, regra.aba_planilha.as_deref().unwrap_or("")),
                    "word" => extractor::extrair_word(&caminho_real),
                    "astah" | "uml" => fs::read_to_string(&caminho_real).map_err(|_| String::from("Arquivo binário.")),
                    _ => Err(String::from("Tipo de arquivo não suportado.")),
                };

                if let Ok(dados_brutos) = resultado_extracao {
                    let mut status_extraido = String::new();
                    let mut desvio_extraido = String::new();
                    let mut sugestao_extraida = String::new();
                    let mut resposta_completa = String::new();
                    
                    loop {
                        if identidade_projeto.is_empty() && regra.id_regra.to_lowercase().contains("visão") {
                            let payload_id = PayloadOllama { model: configuracao.modelo_slm.clone(), prompt: format!("Resuma o sistema:\n{}", dados_brutos), stream: false, options: OllamaOptions { num_ctx: 4096, num_predict: 50, temperature: 0.1, top_k: 10, top_p: 0.5 } };
                            if let Ok(res_id) = cliente_http.post(&configuracao.endpoint_slm).json(&payload_id).send().await {
                                if let Ok(json_id) = res_id.json::<serde_json::Value>().await {
                                    if let Some(texto_id) = json_id.get("response").and_then(|r| r.as_str()) {
                                        identidade_projeto = texto_id.trim().to_string();
                                        conexao_db.execute("INSERT INTO identidade_projeto (projeto, resumo_identidade) VALUES (?1, ?2)", (&sigla_projeto, &identidade_projeto)).ok();
                                    }
                                }
                            }
                        }

                        let regra_plagio = if identidade_projeto.is_empty() { String::new() } else { format!("IDENTIDADE: '{}'. Reprove se fugir do contexto.\n", identidade_projeto) };
                        let prompt_final = format!("=== HISTÓRICO ===\n{}\n{}\n=== CONTEÚDO ===\n{}\n\n=== INSTRUÇÕES ===\n{}", memoria_do_projeto, regra_plagio, dados_brutos, regra.prompt_ia);
                        let contexto_dinamico = ((memoria_do_projeto.len() + dados_brutos.len()) / 3) as u32 + 2048;
                        let payload = PayloadOllama { model: configuracao.modelo_slm.clone(), prompt: prompt_final, stream: true, options: OllamaOptions { num_ctx: contexto_dinamico, num_predict: 150, temperature: 0.0, top_k: 10, top_p: 0.5 } };
                        
                        if let Ok(res) = cliente_http.post(&configuracao.endpoint_slm).json(&payload).send().await {
                            let mut stream = res.bytes_stream();
                            resposta_completa.clear(); 
                            let frames = ["Processando.  ", "Processando.. ", "Processando..."];
                            let mut animacao = 0;

                            while let Some(pedaco_resultado) = stream.next().await {
                                if let Ok(json_parcial) = serde_json::from_slice::<serde_json::Value>(&pedaco_resultado.unwrap_or_default()) {
                                    if let Some(texto) = json_parcial.get("response").and_then(|r| r.as_str()) {
                                        if is_quiet {
                                            if animacao % 5 == 0 { print!("\r{}", frames[(animacao / 5) % 3]); io::stdout().flush().unwrap(); }
                                            animacao += 1;
                                        } else { print!("{}", texto); io::stdout().flush().unwrap(); }
                                        resposta_completa.push_str(texto);
                                    }
                                }
                            }
                            if is_quiet { print!("\r                \r"); } else { println!("\n------------------"); }

                            if let Some(capturas) = re.captures(&resposta_completa) {
                                desvio_extraido = capturas.get(2).unwrap().as_str().trim().replace("\n", " ");
                                sugestao_extraida = capturas.get(3).unwrap().as_str().trim().replace("\n", " ");
                                status_extraido = capturas.get(4).unwrap().as_str().trim().to_string();
                                memoria_do_projeto.push_str(&format!("- Regra {}: {}\n", regra.id_regra, status_extraido));
                                break; 
                            }
                        } else { break; }
                    }
                    if !status_extraido.is_empty() {
                        buffer_banco_dados.push((sigla_projeto.clone(), arquivo_json.clone(), regra.id_regra.clone(), nome_base_resolvido.clone(), status_extraido, desvio_extraido, sugestao_extraida));
                    }
                }
            } else {
                if regra.obrigatorio {
                    buffer_banco_dados.push((sigla_projeto.clone(), arquivo_json.clone(), regra.id_regra.clone(), nome_base_resolvido.clone(), "Não".to_string(), "Artefato obrigatório ausente.".to_string(), "Criar e salvar na pasta correta.".to_string()));
                } else {
                    buffer_banco_dados.push((sigla_projeto.clone(), arquivo_json.clone(), regra.id_regra.clone(), nome_base_resolvido.clone(), "Não se aplica".to_string(), "Opcional.".to_string(), "Nenhuma.".to_string()));
                }
            }
        }

        let mut arquivos_estranhos = Vec::new();
        let pastas_ig = configuracao.pastas_ignoradas_extracao.unwrap_or_default();
        buscar_arquivos_estranhos(diretorio_alvo_projeto, &arquivos_conhecidos, &pastas_ig, &mut arquivos_estranhos);

        for estranho in arquivos_estranhos {
            buffer_banco_dados.push((sigla_projeto.clone(), arquivo_json.clone(), format!("Arquivo não mapeado: {}", estranho), estranho, "Desconhecido".to_string(), "O arquivo não pertence ao JSON de auditoria.".to_string(), "Mover ou excluir o arquivo.".to_string()));
        }

        if !buffer_banco_dados.is_empty() {
            let mut qtd_valido = 0.0;
            let mut qtd_sim = 0.0;

            for reg in &buffer_banco_dados {
                let st = reg.4.to_lowercase();
                if st.contains("sim") { qtd_sim += 1.0; qtd_valido += 1.0; }
                else if st.contains("não") || st.contains("nao") || st.contains("parcialmente") {
                    if !st.contains("aplica") { qtd_valido += 1.0; }
                }
                conexao_db.execute("INSERT INTO historico_auditoria (projeto, fase_json, id_regra, arquivo, status, desvio, sugestao) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (&reg.0, &reg.1, &reg.2, &reg.3, &reg.4, &reg.5, &reg.6)).ok();
            }

            let indicador = if qtd_valido > 0.0 { (qtd_sim / qtd_valido) * 100.0 } else { 0.0 };

            println!("\n====================================================================================================");
            println!("                                          RELATÓRIO DE CONFORMIDADE ({})", arquivo_json);
            println!("====================================================================================================");
            println!("Projeto   : {}", sigla_projeto);
            println!("Aderência : {:.1}% (Indicador do Processo SpinOff)", indicador);
            println!("----------------------------------------------------------------------------------------------------");
            
            for reg in &buffer_banco_dados {
                println!("[{:<12}] {}", reg.4, reg.2);
                let desvio = reg.5.trim();
                let sugestao = reg.6.trim();
                
                if !desvio.is_empty() && desvio.to_lowercase() != "nenhum" && desvio.to_lowercase() != "nenhuma." {
                    println!("  ↳ Desvio  : {}", desvio);
                }
                if !sugestao.is_empty() && sugestao.to_lowercase() != "nenhuma" && sugestao.to_lowercase() != "nenhuma." {
                    println!("  ↳ Sugestão: {}", sugestao);
                }
                println!("-");
            }
            println!("====================================================================================================\n");

            if args.exportar_csv {
                let caminho_salvar = args.saida_csv.clone().unwrap_or_else(|| {
                    diretorio_alvo_projeto.join(format!("Relatorio_{}_{}.csv", sigla_projeto, arquivo_json.replace(".json", ""))).to_string_lossy().to_string()
                });
                
                let mut csv_content = String::from("Projeto;Fase;Status;Pergunta;Desvio;Sugestao\n");
                for reg in &buffer_banco_dados {
                    csv_content.push_str(&format!("{};{};{};{};{};{}\n",
                        reg.0, reg.1, reg.4,
                        reg.2.replace(";", ","), 
                        reg.5.replace(";", ",").replace("\n", " "), 
                        reg.6.replace(";", ",").replace("\n", " ")
                    ));
                }
                fs::write(&caminho_salvar, csv_content).expect("Falha ao salvar CSV");
                let path_absoluto = fs::canonicalize(&caminho_salvar).unwrap_or_else(|_| PathBuf::from(&caminho_salvar));
                println!("💾 Relatório exportado para Excel com sucesso em:\n   {}", path_absoluto.display());
            }
        }
    }
    if let Some(mut proc) = processo_ia_gerenciado { proc.kill().ok(); }
}