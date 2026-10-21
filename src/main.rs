use clap::Parser;
use std::fs::{self};
use std::path::Path;
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

    #[arg(long, help = "Exporta o relatório gerado para um arquivo HTML elegante com gráficos")]
    html: bool,

    #[arg(long = "pasta-saida", help = "Pasta onde o CSV/HTML será salvo (opcional)")]
    pasta_saida: Option<String>,
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

fn registrar_erro(mensagem: &str) {
    let mut pasta_logs = std::env::current_exe().unwrap();
    pasta_logs.pop();
    pasta_logs = pasta_logs.join("logs");
    std::fs::create_dir_all(&pasta_logs).ok(); // Cria a pasta "logs" se não existir

    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(pasta_logs.join("auditoria_erros.log")) {
        let _ = writeln!(file, "[ERRO] {}", mensagem);
    }
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

fn gerar_codigo_html(projeto: &str, indicador: f32, qtd_sim: f32, qtd_nao: f32, qtd_parcial: f32, qtd_na: f32, tabela_html: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html lang="pt-BR">
<head>
    <meta charset="UTF-8"><title>Relatório VIO - {}</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; background-color: #f4f7f6; color: #333; margin: 0; padding: 20px; }}
        .container {{ max-width: 1200px; margin: 0 auto; background: #fff; padding: 30px; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); }}
        h1 {{ color: #2c3e50; text-align: center; border-bottom: 2px solid #3498db; padding-bottom: 10px; }}
        .dashboard {{ display: flex; justify-content: space-around; align-items: center; margin-bottom: 40px; flex-wrap: wrap; }}
        .chart-container {{ width: 300px; height: 300px; }}
        .metric {{ text-align: center; }}
        .metric h2 {{ font-size: 52px; color: #27ae60; margin: 0; }}
        .metric p {{ font-size: 18px; color: #7f8c8d; font-weight: bold; }}
        table {{ width: 100%; border-collapse: collapse; margin-top: 20px; font-size: 14px; }}
        th, td {{ padding: 12px 15px; border-bottom: 1px solid #ddd; text-align: left; }}
        th {{ background-color: #34495e; color: #fff; text-transform: uppercase; }}
        tr:hover {{ background-color: #f1f1f1; }}
        .sim {{ border-left: 5px solid #2ecc71; }}
        .nao {{ border-left: 5px solid #e74c3c; }}
        .parcial {{ border-left: 5px solid #f1c40f; }}
        .na {{ border-left: 5px solid #bdc3c7; color: #95a5a6; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Auditoria de Processo: {}</h1>
        <div class="dashboard">
            <div class="metric">
                <h2>{:.1}%</h2>
                <p>Aderência do Processo</p>
            </div>
            <div class="chart-container">
                <canvas id="graficoAderencia"></canvas>
            </div>
        </div>
        <table>
            <thead><tr><th>Fase</th><th>Pergunta</th><th>Status</th><th>Desvio</th><th>Sugestão</th></tr></thead>
            <tbody>{}</tbody>
        </table>
    </div>
    <script>
        var ctx = document.getElementById('graficoAderencia').getContext('2d');
        new Chart(ctx, {{
            type: 'doughnut',
            data: {{
                labels: ['Sim', 'Não', 'Parcialmente', 'Não Se Aplica'],
                datasets: [{{ data: [{}, {}, {}, {}], backgroundColor: ['#2ecc71', '#e74c3c', '#f1c40f', '#bdc3c7'] }}]
            }},
            options: {{ responsive: true, maintainAspectRatio: false }}
        }});
    </script>
</body>
</html>"#, projeto, projeto, indicador, tabela_html, qtd_sim, qtd_nao, qtd_parcial, qtd_na)
}

#[tokio::main]
async fn main() {
    let args = ArgumentosCli::parse();
    let mut caminho_executavel = std::env::current_exe().unwrap();
    caminho_executavel.pop();

    // Diretório seguro para escrita de dados no Windows (%LOCALAPPDATA%\VIO)
    let pasta_dados = dirs::data_local_dir()
        .map(|p| p.join("VIO"))
        .unwrap_or_else(|| caminho_executavel.clone());

    if !pasta_dados.exists() {
        let _ = fs::create_dir_all(&pasta_dados);
    }

    let caminho_settings = pasta_dados.join("settings.json");
    let mut settings = if caminho_settings.exists() {
        serde_json::from_str(&fs::read_to_string(&caminho_settings).unwrap_or_default()).unwrap_or(SettingsLocal { quiet_mode: false })
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

    let caminho_banco = pasta_dados.join("auditorias.db");
    let conexao_db = Connection::open(&caminho_banco).unwrap();
    conexao_db.execute("CREATE TABLE IF NOT EXISTS identidade_projeto (projeto TEXT PRIMARY KEY, resumo_identidade TEXT NOT NULL)", []).ok();
    conexao_db.execute("CREATE TABLE IF NOT EXISTS historico_auditoria (id INTEGER PRIMARY KEY AUTOINCREMENT, projeto TEXT NOT NULL, fase_json TEXT NOT NULL, id_regra TEXT NOT NULL, arquivo TEXT NOT NULL, status TEXT NOT NULL, desvio TEXT DEFAULT '', sugestao TEXT DEFAULT '', data_auditoria DATETIME DEFAULT CURRENT_TIMESTAMP)", []).ok();

    if args.limpar_banco {
        conexao_db.execute("DROP TABLE IF EXISTS historico_auditoria", []).ok();
        conexao_db.execute("DROP TABLE IF EXISTS identidade_projeto", []).ok();
        println!("🗑️ Banco de dados limpo!");
        return;
    }

    if args.mostrar_banco {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(ref p) = args.projeto { sql.push_str(" AND projeto = ?"); params.push(p.clone()); }
        if let Some(ref f) = args.fase { sql.push_str(" AND fase_json = ?"); params.push(f.clone()); }
        if let Some(ref r) = args.id_regra { sql.push_str(" AND id_regra LIKE ?"); params.push(format!("%{}%", r)); }
        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(format!("PROJ: {} | STATUS: {:<12} | REGRA: {}\n -> DESVIO: {}\n -> SUGESTÃO: {}\n", row.get::<_, String>(0)?, row.get::<_, String>(3)?, row.get::<_, String>(2)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
        }).unwrap();
        for audit in auditorias { println!("{}", audit.unwrap()); }
        return;
    }

    if args.update {
        println!("🔄 Atualizando motor da IA (Phi-3)...");
        Command::new("ollama").arg("pull").arg("phi3").status().ok();
        
        println!("\n🔄 Buscando atualizações do VIO no GitHub...");
        
        // ATENÇÃO: Substitua "SeuUsuario" e "NomeDoRepositorio" pelos reais no GitHub
        let status_update = self_update::backends::github::Update::configure()
            .repo_owner("GaTimpone") 
            .repo_name("VIO---VerInOff---Verifica-o-In-Off")
            .bin_name("vio") // O nome do seu .exe no release do GitHub
            .show_download_progress(true)
            .current_version(env!("CARGO_PKG_VERSION")) // Puxa a versão do seu Cargo.toml
            .build();

        match status_update {
            Ok(updater) => {
                match updater.update() {
                    Ok(status) => {
                        if status.updated() {
                            println!("🎉 VIO atualizado com sucesso para a versão: {}", status.version());
                        } else {
                            println!("✅ Você já possui a versão mais recente ({}).", status.version());
                        }
                    }
                    Err(e) => registrar_erro(&format!("Falha ao aplicar atualização: {}", e)),
                }
            }
            Err(e) => registrar_erro(&format!("Falha ao contatar o GitHub: {}", e)),
        }
        return;
    }

    if args.gerar_relatorio {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(ref p) = args.projeto { sql.push_str(" AND projeto = ?"); params.push(p.clone()); }
        if let Some(ref f) = args.fase { sql.push_str(" AND fase_json = ?"); params.push(f.clone()); }

        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
        }).unwrap();

        let mut registros = Vec::new();
        for audit in auditorias { registros.push(audit.unwrap()); }

        if registros.is_empty() {
            println!("Nenhum registro encontrado para os filtros informados.");
            return;
        }

        let mut qtd_valido = 0.0; let mut qtd_sim = 0.0; let mut qtd_nao = 0.0; let mut qtd_parcial = 0.0; let mut qtd_na = 0.0;
        let mut html_linhas = String::new();

        for reg in &registros {
            let st = reg.3.to_lowercase();
            let mut classe = "na";
            if st.contains("sim") { qtd_sim += 1.0; qtd_valido += 1.0; classe = "sim"; }
            else if st.contains("parcialmente") { qtd_parcial += 1.0; qtd_valido += 1.0; classe = "parcial"; }
            else if st.contains("não") || st.contains("nao") {
                if st.contains("aplica") { qtd_na += 1.0; } else { qtd_nao += 1.0; qtd_valido += 1.0; classe = "nao"; }
            }
            html_linhas.push_str(&format!("<tr class='{}'><td>{}</td><td>{}</td><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>\n", classe, reg.1, reg.2, reg.3, reg.4.trim(), reg.5.trim()));
        }

        let indicador = if qtd_valido > 0.0 { (qtd_sim / qtd_valido) * 100.0 } else { 0.0 };

        println!("\n====================================================================================================");
        println!("                                  RELATÓRIO DE CONFORMIDADE (VIA BANCO)");
        println!("====================================================================================================");
        println!("Aderência : {:.1}% (Indicador do Processo SpinOff)", indicador);
        println!("----------------------------------------------------------------------------------------------------");
        for reg in &registros {
            println!("[{:<12}] {}", reg.3, reg.2);
            let desvio = reg.4.trim(); let sugestao = reg.5.trim();
            if !desvio.is_empty() && desvio.to_lowercase() != "nenhum" && desvio.to_lowercase() != "nenhuma." { println!("  ↳ Desvio  : {}", desvio); }
            if !sugestao.is_empty() && sugestao.to_lowercase() != "nenhuma" && sugestao.to_lowercase() != "nenhuma." { println!("  ↳ Sugestão: {}", sugestao); }
            println!("-");
        }
        println!("====================================================================================================\n");

        if args.exportar_csv || args.html {
            let pasta_destino = args.pasta_saida.clone().map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap().join("relatorios"));
            if !pasta_destino.exists() { fs::create_dir_all(&pasta_destino).unwrap(); }
            
            let proj_nome = args.projeto.unwrap_or_else(|| "Geral".to_string());

            if args.exportar_csv {
                let mut csv_content = format!("\u{FEFF}Aderência do Processo: {:.1}%\n\nProjeto;Fase;Status;Pergunta;Desvio;Sugestão\n", indicador);
                for reg in &registros {
                    csv_content.push_str(&format!("{};{};{};{};{};{}\n", reg.0, reg.1, reg.3, reg.2.replace(";", ","), reg.4.replace(";", ",").replace("\n", " "), reg.5.replace(";", ",").replace("\n", " ")));
                }
                let caminho_csv = pasta_destino.join(format!("Relatorio_{}.csv", proj_nome));
                fs::write(&caminho_csv, csv_content).unwrap();
                println!("💾 CSV exportado com sucesso: {}", fs::canonicalize(&caminho_csv).unwrap_or(caminho_csv).display());
            }

            if args.html {
                let html_content = gerar_codigo_html(&proj_nome, indicador, qtd_sim, qtd_nao, qtd_parcial, qtd_na, &html_linhas);
                let caminho_html = pasta_destino.join(format!("Dashboard_{}.html", proj_nome));
                fs::write(&caminho_html, html_content).unwrap();
                println!("📊 Dashboard HTML exportado com sucesso: {}", fs::canonicalize(&caminho_html).unwrap_or(caminho_html).display());
            }
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
        let conteudo_json = fs::read_to_string(&caminho_json).unwrap();
        
        // Proteção contra JSONs quebrados, antigos ou com campos faltando
        let configuracao: ConfiguracaoGlobal = match serde_json::from_str(&conteudo_json) {
            Ok(config) => config,
            Err(erro) => {
                registrar_erro(&format!("Erro ao ler JSON '{}': {}", arquivo_json, erro));
                if !is_quiet { println!("⚠️ Pulando o arquivo '{}': Erro na estrutura do JSON.", arquivo_json); }
                continue;
            }
        };
        
        if processo_ia_gerenciado.is_none() { processo_ia_gerenciado = gerenciar_motor_ia(&configuracao.endpoint_slm); }

        let mut arquivos_conhecidos = HashSet::new();
        let mut buffer_banco_dados: Vec<(String, String, String, String, String, String, String)> = Vec::new();
        let mut memoria_do_projeto = String::new();
        let mut identidade_projeto: String = conexao_db.query_row("SELECT resumo_identidade FROM identidade_projeto WHERE projeto = ?1", [&sigla_projeto], |row| row.get(0)).unwrap_or_default();

        // Regex flexível: aceita "ANALISAÇÃO", "SUGÉSTIMO" e afins, e pega apenas a primeira ocorrência.
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
                        let payload = PayloadOllama { model: configuracao.modelo_slm.clone(), prompt: prompt_final, stream: true, options: OllamaOptions { num_ctx: contexto_dinamico, num_predict: 300, temperature: 0.1, top_k: 10, top_p: 0.5 } };
                        
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
                        } else { 
                            registrar_erro(&format!("Falha na comunicação HTTP com a IA. Regra: {}", regra.id_regra));
                            break; 
                        }
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
            let mut qtd_valido = 0.0; let mut qtd_sim = 0.0; let mut qtd_nao = 0.0; let mut qtd_parcial = 0.0; let mut qtd_na = 0.0;
            let mut html_linhas = String::new();

            for reg in &buffer_banco_dados {
                let st = reg.4.to_lowercase();
                let mut classe = "na";
                if st.contains("sim") { qtd_sim += 1.0; qtd_valido += 1.0; classe = "sim"; }
                else if st.contains("parcialmente") { qtd_parcial += 1.0; qtd_valido += 1.0; classe = "parcial"; }
                else if st.contains("não") || st.contains("nao") {
                    if st.contains("aplica") { qtd_na += 1.0; } else { qtd_nao += 1.0; qtd_valido += 1.0; classe = "nao"; }
                }
                conexao_db.execute("INSERT INTO historico_auditoria (projeto, fase_json, id_regra, arquivo, status, desvio, sugestao) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (&reg.0, &reg.1, &reg.2, &reg.3, &reg.4, &reg.5, &reg.6)).ok();
                
                html_linhas.push_str(&format!("<tr class='{}'><td>{}</td><td>{}</td><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>\n", classe, reg.1, reg.2, reg.4, reg.5.trim(), reg.6.trim()));
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
                let desvio = reg.5.trim(); let sugestao = reg.6.trim();
                if !desvio.is_empty() && desvio.to_lowercase() != "nenhum" && desvio.to_lowercase() != "nenhuma." { println!("  ↳ Desvio  : {}", desvio); }
                if !sugestao.is_empty() && sugestao.to_lowercase() != "nenhuma" && sugestao.to_lowercase() != "nenhuma." { println!("  ↳ Sugestão: {}", sugestao); }
                println!("-");
            }
            println!("====================================================================================================\n");

            if args.exportar_csv || args.html {
                let pasta_destino = args.pasta_saida.clone().map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap().join("relatorios"));
                if !pasta_destino.exists() { fs::create_dir_all(&pasta_destino).unwrap(); }

                if args.exportar_csv {
                    let mut csv_content = format!("\u{FEFF}Aderência do Processo: {:.1}%\n\nProjeto;Fase;Status;Pergunta;Desvio;Sugestão\n", indicador);
                    for reg in &buffer_banco_dados {
                        csv_content.push_str(&format!("{};{};{};{};{};{}\n", reg.0, reg.1, reg.4, reg.2.replace(";", ","), reg.5.replace(";", ",").replace("\n", " "), reg.6.replace(";", ",").replace("\n", " ")));
                    }
                    let caminho_csv = pasta_destino.join(format!("Relatorio_{}_{}.csv", sigla_projeto, arquivo_json.replace(".json", "")));
                    fs::write(&caminho_csv, csv_content).unwrap();
                    println!("💾 CSV exportado com sucesso: {}", fs::canonicalize(&caminho_csv).unwrap_or(caminho_csv).display());
                }

                if args.html {
                    let html_content = gerar_codigo_html(&sigla_projeto, indicador, qtd_sim, qtd_nao, qtd_parcial, qtd_na, &html_linhas);
                    let caminho_html = pasta_destino.join(format!("Dashboard_{}_{}.html", sigla_projeto, arquivo_json.replace(".json", "")));
                    fs::write(&caminho_html, html_content).unwrap();
                    println!("📊 Dashboard HTML exportado com sucesso: {}", fs::canonicalize(&caminho_html).unwrap_or(caminho_html).display());
                }
            }
        }
    }
    if let Some(mut proc) = processo_ia_gerenciado { proc.kill().ok(); }
}