use clap::Parser;
use futures_util::StreamExt;
use regex::Regex;
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

mod config;
mod extractor;

use config::{ConfiguracaoGlobal, SettingsLocal};

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

    #[arg(
        long = "ler-astah",
        num_args = 0..=1,
        default_missing_value = "",
        help = "Extrai e exibe no terminal os diagramas, casos de uso, descrições e classes do arquivo Astah (.asta)"
    )]
    ler_astah: Option<String>,
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

/// Obtém o diretório seguro de dados locais (%LOCALAPPDATA%\VIO no Windows)
fn obter_pasta_dados() -> PathBuf {
    let mut caminho_executavel = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    caminho_executavel.pop();
    dirs::data_local_dir()
        .map(|p| p.join("VIO"))
        .unwrap_or(caminho_executavel)
}

/// Registra erros em %LOCALAPPDATA%\VIO\logs\auditoria_erros.log (evitando falha de permissão em Program Files)
fn registrar_erro(mensagem: &str) {
    let pasta_logs = obter_pasta_dados().join("logs");
    let _ = fs::create_dir_all(&pasta_logs);

    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(pasta_logs.join("auditoria_erros.log"))
    {
        let _ = writeln!(file, "[ERRO] {}", mensagem);
    }
}

/// Localiza o executável do Ollama no PATH ou no diretório padrão de instalação do Windows
fn encontrar_binario_ollama() -> String {
    if Command::new("ollama")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return "ollama".to_string();
    }
    if let Some(local_app_data) = dirs::data_local_dir() {
        let caminho_padrao = local_app_data.join("Programs").join("Ollama").join("ollama.exe");
        if caminho_padrao.exists() {
            return caminho_padrao.to_string_lossy().to_string();
        }
    }
    "ollama".to_string()
}

/// Verifica se o servidor Ollama está ativo; caso contrário, tenta iniciá-lo em segundo plano
fn gerenciar_motor_ia(endpoint: &str) -> Option<Child> {
    let endereco_limpo = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("127.0.0.1:11434")
        .replace("localhost", "127.0.0.1");

    let socket_addr: SocketAddr = endereco_limpo
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:11434".parse().unwrap());

    if TcpStream::connect_timeout(&socket_addr, Duration::from_secs(1)).is_ok() {
        return None;
    }

    let ollama_bin = encontrar_binario_ollama();
    if let Ok(child) = Command::new(&ollama_bin)
        .arg("serve")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        thread::sleep(Duration::from_secs(4));
        return Some(child);
    }
    None
}

/// Normaliza texto removendo acentos, pontuação e caixa alta para comparação tolerante de nomes de arquivos e pastas
fn normalizar_texto(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            '-' | '_' | '.' => ' ',
            other => other,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normaliza palavras no plural/variações comuns do SpinOff (ex: "modelos" -> "modelo", "testes" -> "teste")
fn extrair_palavras_chave_artefato(nome_base_resolvido: &str, sigla_projeto: &str) -> Vec<String> {
    let norm_total = normalizar_texto(nome_base_resolvido);
    let norm_sigla = normalizar_texto(sigla_projeto);

    // Remove a sigla do início para focar no nome do artefato em si
    let sem_sigla = if !norm_sigla.is_empty() && norm_total.starts_with(&norm_sigla) {
        norm_total[norm_sigla.len()..].trim().to_string()
    } else {
        norm_total
    };

    let stopwords = ["de", "do", "da", "dos", "das", "e", "o", "a", "os", "as", "em", "no", "na"];

    sem_sigla
        .split_whitespace()
        .filter(|p| !stopwords.contains(p) && !p.is_empty())
        .map(|p| match p {
            "modelos" => "modelo".to_string(),
            "testes" => "teste".to_string(),
            "historias" => "historia".to_string(),
            "usuarios" => "usuario".to_string(),
            "atas" => "ata".to_string(),
            "reunioes" => "reuniao".to_string(),
            "prototipos" => "prototipo".to_string(),
            "guias" => "guia".to_string(),
            "manuais" => "manual".to_string(),
            "roteiros" => "roteiro".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Resolve o caminho de uma subpasta esperada mesmo que haja diferença de acentuação ou maiúsculas/minúsculas (ex: "5.Implantacao" vs "5.Implantação")
fn resolver_pasta_alvo(diretorio_base: &Path, pasta_esperada: &str) -> PathBuf {
    let caminho_direto = diretorio_base.join(pasta_esperada);
    if caminho_direto.exists() {
        return caminho_direto;
    }

    let mut atual = diretorio_base.to_path_buf();
    for segmento in pasta_esperada.split(|c| c == '/' || c == '\\').filter(|s| !s.is_empty()) {
        let tentativa = atual.join(segmento);
        if tentativa.exists() {
            atual = tentativa;
            continue;
        }
        let seg_norm = normalizar_texto(segmento);
        let mut encontrado = None;
        if let Ok(entradas) = fs::read_dir(&atual) {
            for entrada in entradas.flatten() {
                let p = entrada.path();
                if p.is_dir() {
                    if let Some(nome) = p.file_name() {
                        if normalizar_texto(&nome.to_string_lossy()) == seg_norm {
                            encontrado = Some(p);
                            break;
                        }
                    }
                }
            }
        }
        if let Some(dir_real) = encontrado {
            atual = dir_real;
        } else {
            atual = tentativa;
        }
    }
    atual
}

/// Verifica se a extensão do arquivo é compatível com o tipo esperado pela regra
fn extensao_compativel(caminho: &Path, tipo_arquivo: &str, apenas_existencia: bool) -> bool {
    let ext = caminho
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    if apenas_existencia {
        // Para regras de existência (como Protótipo), aceita documentos, imagens, PDFs, etc.
        return matches!(
            ext.as_str(),
            "docx" | "doc" | "xlsx" | "xls" | "asta" | "uml" | "png" | "jpg" | "jpeg" | "pdf" | "fig" | "html" | "bmpr"
        );
    }

    match tipo_arquivo.to_lowercase().as_str() {
        "word" => matches!(ext.as_str(), "docx" | "doc"),
        "excel" => matches!(ext.as_str(), "xlsx" | "xls" | "xlsm"),
        "uml" | "astah" => matches!(ext.as_str(), "asta" | "uml" | "puml" | "xmi"),
        _ => true,
    }
}

/// Encontra todos os arquivos válidos na pasta alvo que correspondem ao artefato esperado pela regra
/// Suporta múltiplos arquivos para o mesmo artefato (ex: várias Atas de Reunião com datas ou várias Histórias de Usuário UC01..UC08)
fn encontrar_arquivos_regra(
    pasta_alvo: &Path,
    nome_base_resolvido: &str,
    sigla_projeto: &str,
    tipo_arquivo: &str,
    apenas_existencia: bool,
) -> Vec<PathBuf> {
    let mut encontrados = Vec::new();
    if !pasta_alvo.exists() || !pasta_alvo.is_dir() {
        return encontrados;
    }

    let alvo_norm = normalizar_texto(nome_base_resolvido);
    let palavras_chave = extrair_palavras_chave_artefato(nome_base_resolvido, sigla_projeto);

    if let Ok(entradas) = fs::read_dir(pasta_alvo) {
        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            if !caminho.is_file() {
                continue;
            }

            let nome_arquivo = caminho
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Ignora arquivos temporários de bloqueio do Office (~$), arquivos ocultos e templates (.dotx/.xltx)
            if nome_arquivo.starts_with("~$")
                || nome_arquivo.starts_with('.')
                || nome_arquivo.to_lowercase().starts_with("template")
            {
                continue;
            }

            if !extensao_compativel(&caminho, tipo_arquivo, apenas_existencia) {
                continue;
            }

            let stem = caminho
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let stem_norm = normalizar_texto(&stem);
            let stem_palavras = extrair_palavras_chave_artefato(&stem, sigla_projeto);

            // 1. Correspondência direta normalizada (ignorando acentos, caixa e hífens)
            let match_direto = stem_norm == alvo_norm;

            // 2. Correspondência por palavras-chave do artefato (ex: "HH - UC01 - História de Usuário" contém ["historia", "usuario"])
            let match_palavras = !palavras_chave.is_empty()
                && palavras_chave
                    .iter()
                    .all(|kw| stem_palavras.iter().any(|sp| sp == kw || sp.starts_with(kw)));

            if match_direto || match_palavras {
                encontrados.push(caminho);
            }
        }
    }

    encontrados.sort();
    encontrados
}

/// Verifica se um arquivo é um arquivo auxiliar/sistema ou artefato padrão de outra fase do SpinOff que não deve ser acusado como "estranho"
fn eh_arquivo_sistema_ou_padrao_spinoff(caminho: &Path) -> bool {
    let nome = caminho
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let nome_lower = nome.to_lowercase();

    // Arquivos temporários do Office, ocultos do Git/SO, README ou Templates do SpinOff
    if nome.starts_with("~$")
        || nome.starts_with('.')
        || nome_lower == "thumbs.db"
        || nome_lower == "desktop.ini"
        || nome_lower == "readme.md"
        || nome_lower.starts_with("template ")
        || nome_lower.starts_with("template-")
        || nome_lower.ends_with(".dotx")
        || nome_lower.ends_with(".xltx")
    {
        return true;
    }

    // Artefatos legítimos do processo SpinOff que coexistem nas pastas do projeto
    let norm = normalizar_texto(&nome);
    if norm.contains("checklist") && (norm.contains("projeto") || norm.contains("verificacao")) {
        return true;
    }
    if norm.contains("massa de teste") {
        return true;
    }
    if norm.contains("modelo") && norm.contains("analise") && norm.contains("design") {
        return true;
    }
    if norm.contains("prototipo") {
        return true;
    }
    if norm.contains("historia") && norm.contains("usuario") {
        return true;
    }
    if norm.contains("especificacao") && norm.contains("caso") && norm.contains("uso") {
        return true;
    }
    if norm.contains("glossario") {
        return true;
    }
    if norm.contains("roteiro") && norm.contains("teste") {
        return true;
    }
    if norm.contains("guia") && norm.contains("implantacao") {
        return true;
    }
    if norm.contains("manual") && norm.contains("usuario") {
        return true;
    }

    false
}

fn buscar_arquivos_estranhos(
    dir: &Path,
    arquivos_conhecidos: &HashSet<String>,
    ignoradas: &[String],
    achados: &mut Vec<String>,
) {
    if let Ok(entradas) = fs::read_dir(dir) {
        for entrada in entradas.flatten() {
            let path = entrada.path();
            let nome_entrada = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            if path.is_dir() {
                let pasta_norm = normalizar_texto(&nome_entrada);
                let deve_ignorar = nome_entrada.starts_with('.')
                    || ignoradas.iter().any(|ig| {
                        let ig_norm = normalizar_texto(ig);
                        nome_entrada.contains(ig) || (!ig_norm.is_empty() && pasta_norm.contains(&ig_norm))
                    });
                if deve_ignorar {
                    continue;
                }
                buscar_arquivos_estranhos(&path, arquivos_conhecidos, ignoradas, achados);
            } else if path.is_file() {
                if eh_arquivo_sistema_ou_padrao_spinoff(&path) {
                    continue;
                }
                let caminho_str = path.to_string_lossy().replace('\\', "/");
                if !arquivos_conhecidos.contains(&caminho_str) {
                    achados.push(caminho_str);
                }
            }
        }
    }
}

/// Normaliza o Status Qualitativo extraído da IA para os 4 valores canônicos do SpinOff
fn normalizar_status_qualitativo(status_bruto: &str) -> String {
    let s = normalizar_texto(status_bruto);
    if s.contains("aplica") || s == "na" || s == "n a" || s == "n/a" {
        "Não se aplica".to_string()
    } else if s.contains("parcial") {
        "Parcialmente".to_string()
    } else if s.contains("sim") {
        "Sim".to_string()
    } else if s.contains("nao") {
        "Não".to_string()
    } else {
        "Parcialmente".to_string()
    }
}

/// Verifica deterministicamente se o campo Desvio afirma que não há desvios no artefato auditado
fn eh_desvio_inexistente(desvio: &str) -> bool {
    let d = normalizar_texto(desvio);
    d.starts_with("nenhum")
        || d.starts_with("nenhuma")
        || d.starts_with("sem desvio")
        || d.starts_with("nao ha desvio")
        || d.starts_with("nao ha problema")
        || d.starts_with("nao houve desvio")
        || d.starts_with("nada a declarar")
}

/// Coleta recursivamente nomes de arquivos de código-fonte dentro de '3.Implementação' para comparação cruzada com o Diagrama de Classes do Astah
fn coletar_arquivos_codigo_implementacao(dir: &Path, lista: &mut Vec<String>) {
    if lista.len() >= 80 {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if lista.len() >= 80 {
                break;
            }
            let path = entry.path();
            let nome = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if path.is_dir() {
                let n_low = nome.to_lowercase();
                if n_low == "node_modules"
                    || n_low == "target"
                    || n_low == ".git"
                    || n_low == "dist"
                    || n_low == "build"
                    || n_low == ".idea"
                    || n_low == ".vscode"
                {
                    continue;
                }
                coletar_arquivos_codigo_implementacao(&path, lista);
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if matches!(
                    ext.as_str(),
                    "java" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "cs" | "php" | "kt" | "go" | "cpp" | "c" | "html" | "sql"
                ) {
                    lista.push(nome);
                }
            }
        }
    }
}

/// Quando uma regra da checklist exige comparação cruzada entre dois artefatos distintos (ex: Backlog x Casos de Uso do Astah,
/// Roteiro de Testes x Casos de Uso do Astah/Backlog, Riscos x Restrições do Visão, ou Diagrama de Classes x Código-Fonte),
/// extrai automaticamente o artefato de referência para que a auditoria compare os dois lado a lado.
fn obter_contexto_cruzado_comparacao(
    diretorio_projeto: &Path,
    sigla_projeto: &str,
    id_regra: &str,
) -> String {
    let regra_norm = normalizar_texto(id_regra);
    let mut extra = String::new();

    // 1. Regras que comparam Backlog do Produto ou Roteiro de Testes com os Casos de Uso do Modelo Astah (.asta)
    if regra_norm.contains("backlog do produto foi criado com todos os casos de uso")
        || regra_norm.contains("roteiro com casos de testes para cada caso de uso")
    {
        let pasta_uml = resolver_pasta_alvo(diretorio_projeto, "2.Analise e Design");
        let base_uml = format!("{} - Modelo de Análise e Design", sigla_projeto);
        let arqs_uml = encontrar_arquivos_regra(&pasta_uml, &base_uml, sigla_projeto, "uml", false);
        for arq in arqs_uml {
            if let Ok(texto_uml) = extractor::extrair_astah(&arq) {
                extra.push_str("\n\n=== MODELO UML ASTAH EXTRAÍDO DE '2.Analise e Design' (REFERÊNCIA PARA COMPARAÇÃO CRUZADA DE CASOS DE USO) ===\n");
                extra.push_str(&texto_uml);
                break;
            }
        }
    }

    // 2. Regra do Roteiro de Testes também compara com a lista de Casos de Uso do Backlog_Produto
    if regra_norm.contains("roteiro com casos de testes para cada caso de uso") {
        let pasta_plan = resolver_pasta_alvo(diretorio_projeto, "6.Gerenciamento de Projeto");
        let base_plan = format!("{} - Planejamento e Controle do Projeto", sigla_projeto);
        let arqs_plan =
            encontrar_arquivos_regra(&pasta_plan, &base_plan, sigla_projeto, "excel", false);
        for arq in arqs_plan {
            if let Ok(texto_backlog) = extractor::extrair_excel(&arq, "Backlog_Produto") {
                extra.push_str("\n\n=== CASOS DE USO NO BACKLOG DO PRODUTO (REFERÊNCIA PARA COMPARAÇÃO CRUZADA COM O ROTEIRO DE TESTES) ===\n");
                extra.push_str(&texto_backlog);
                break;
            }
        }
    }

    // 3. Regra de Riscos que compara com as Restrições registradas no Documento Visão (.docx)
    if regra_norm.contains("plano de gerenciamento de riscos considerou as restricoes registradas no visao") {
        let pasta_req = resolver_pasta_alvo(diretorio_projeto, "1.Requisitos");
        let base_visao = format!("{} - Visão", sigla_projeto);
        let arqs_visao =
            encontrar_arquivos_regra(&pasta_req, &base_visao, sigla_projeto, "word", false);
        for arq in arqs_visao {
            if let Ok(texto_visao) = extractor::extrair_word(&arq) {
                extra.push_str("\n\n=== DOCUMENTO VISÃO EXTRAÍDO DE '1.Requisitos' (REFERÊNCIA PARA COMPARAÇÃO CRUZADA DAS RESTRIÇÕES COM OS RISCOS) ===\n");
                extra.push_str(&texto_visao);
                break;
            }
        }
    }

    // 4. Regras de Construção que comparam o Diagrama de Classes do Astah com a Codificação em '3.Implementação'
    if regra_norm.contains("diagramas de classe foram atualizados conforme a codificacao")
        || regra_norm.contains("diagramas de casos de uso e classes foram atualizados conforme a implementacao")
    {
        let pasta_impl = resolver_pasta_alvo(diretorio_projeto, "3.Implementacao");
        let mut arqs_codigo = Vec::new();
        coletar_arquivos_codigo_implementacao(&pasta_impl, &mut arqs_codigo);
        if !arqs_codigo.is_empty() {
            extra.push_str("\n\n=== ARQUIVOS DE CÓDIGO-FONTE ENCONTRADOS EM '3.Implementação' (REFERÊNCIA PARA COMPARAÇÃO CRUZADA COM O MODELO UML) ===\n");
            extra.push_str(&arqs_codigo.join(", "));
            extra.push('\n');
        }
    }

    extra
}

fn gerar_codigo_html(
    projeto: &str,
    indicador: f32,
    qtd_sim: f32,
    qtd_nao: f32,
    qtd_parcial: f32,
    qtd_na: f32,
    tabela_html: &str,
) -> String {
    format!(
        r#"<!DOCTYPE html>
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
</html>"#,
        projeto, projeto, indicador, tabela_html, qtd_sim, qtd_nao, qtd_parcial, qtd_na
    )
}

#[tokio::main]
async fn main() {
    let args = ArgumentosCli::parse();
    let mut caminho_executavel = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    caminho_executavel.pop();

    // Diretório seguro para escrita de dados no Windows (%LOCALAPPDATA%\VIO)
    let pasta_dados = obter_pasta_dados();
    if !pasta_dados.exists() {
        let _ = fs::create_dir_all(&pasta_dados);
    }

    let caminho_settings = pasta_dados.join("settings.json");
    let mut settings = if caminho_settings.exists() {
        serde_json::from_str(&fs::read_to_string(&caminho_settings).unwrap_or_default())
            .unwrap_or(SettingsLocal { quiet_mode: false })
    } else {
        SettingsLocal { quiet_mode: false }
    };

    if args.toggle_quiet {
        settings.quiet_mode = !settings.quiet_mode;
        fs::write(&caminho_settings, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        println!(
            "⚙️ Modo Silencioso alterado para: {}",
            if settings.quiet_mode { "ATIVADO" } else { "DESATIVADO" }
        );
        return;
    }

    if let Some(ref alvo_astah) = args.ler_astah {
        let caminho_busca = if !alvo_astah.trim().is_empty() {
            Path::new(alvo_astah.trim())
        } else {
            Path::new(&args.caminho_projeto)
        };

        let mut arquivos_asta: Vec<PathBuf> = Vec::new();
        if caminho_busca.is_file() {
            arquivos_asta.push(caminho_busca.to_path_buf());
        } else if caminho_busca.is_dir() {
            fn buscar_asta_recursivo(dir: &Path, achados: &mut Vec<PathBuf>) {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            buscar_asta_recursivo(&p, achados);
                        } else if p.is_file() {
                            let ext = p
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            if ext == "asta" || ext == "uml" || ext == "puml" || ext == "xmi" {
                                achados.push(p);
                            }
                        }
                    }
                }
            }
            buscar_asta_recursivo(caminho_busca, &mut arquivos_asta);
        }

        if arquivos_asta.is_empty() {
            println!(
                "⚠️ Nenhum arquivo Astah (.asta) ou UML foi encontrado em '{}'.",
                caminho_busca.display()
            );
            return;
        }

        for arq in arquivos_asta {
            let tamanho_kb = fs::metadata(&arq).map(|m| m.len() / 1024).unwrap_or(0);
            println!("====================================================================================================");
            println!(
                "🔍 INSPEÇÃO BINÁRIA NATIVA ASTAH (.ASTA) — VIO v{}",
                env!("CARGO_PKG_VERSION")
            );
            println!("Arquivo : {} ({} KB)", arq.display(), tamanho_kb);
            println!("====================================================================================================");
            match extractor::extrair_astah(&arq) {
                Ok(conteudo) => println!("{}", conteudo),
                Err(erro) => println!("❌ Erro ao extrair modelo Astah: {}", erro),
            }
            println!("====================================================================================================");
        }
        return;
    }

    let is_quiet = settings.quiet_mode;

    let caminho_banco = pasta_dados.join("auditorias.db");
    let conexao_db = Connection::open(&caminho_banco).unwrap();
    conexao_db
        .execute(
            "CREATE TABLE IF NOT EXISTS identidade_projeto (projeto TEXT PRIMARY KEY, resumo_identidade TEXT NOT NULL)",
            [],
        )
        .ok();
    conexao_db
        .execute(
            "CREATE TABLE IF NOT EXISTS historico_auditoria (id INTEGER PRIMARY KEY AUTOINCREMENT, projeto TEXT NOT NULL, fase_json TEXT NOT NULL, id_regra TEXT NOT NULL, arquivo TEXT NOT NULL, status TEXT NOT NULL, desvio TEXT DEFAULT '', sugestao TEXT DEFAULT '', data_auditoria DATETIME DEFAULT CURRENT_TIMESTAMP)",
            [],
        )
        .ok();

    if args.limpar_banco {
        conexao_db.execute("DROP TABLE IF EXISTS historico_auditoria", []).ok();
        conexao_db.execute("DROP TABLE IF EXISTS identidade_projeto", []).ok();
        println!("🗑️ Banco de dados limpo!");
        return;
    }

    if args.mostrar_banco {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(ref p) = args.projeto {
            sql.push_str(" AND projeto = ?");
            params.push(p.clone());
        }
        if let Some(ref f) = args.fase {
            sql.push_str(" AND fase_json = ?");
            params.push(f.clone());
        }
        if let Some(ref r) = args.id_regra {
            sql.push_str(" AND id_regra LIKE ?");
            params.push(format!("%{}%", r));
        }
        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok(format!(
                    "PROJ: {} | FASE: {} | STATUS: {:<13} | REGRA: {}\n -> DESVIO: {}\n -> SUGESTÃO: {}\n",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?
                ))
            })
            .unwrap();
        for audit in auditorias {
            println!("{}", audit.unwrap());
        }
        return;
    }

    if args.update {
        println!("🔄 Atualizando motor da IA (Phi-3)...");
        let ollama_bin = encontrar_binario_ollama();
        Command::new(&ollama_bin).arg("pull").arg("phi3").status().ok();

        println!("\n🔄 Buscando atualizações do VIO no GitHub...");

        let status_update = self_update::backends::github::Update::configure()
            .repo_owner("GaTimpone")
            .repo_name("VIO---VerInOff---Verifica-o-In-Off")
            .bin_name("vio")
            .target(std::env::consts::OS)
            .show_download_progress(true)
            .current_version(env!("CARGO_PKG_VERSION"))
            .build();

        match status_update {
            Ok(updater) => match updater.update() {
                Ok(status) => {
                    if status.updated() {
                        println!("🎉 VIO atualizado com sucesso para a versão: {}", status.version());
                    } else {
                        println!("✅ Você já possui a versão mais recente ({}).", status.version());
                    }
                }
                Err(e) => registrar_erro(&format!("Falha ao aplicar atualização: {}", e)),
            },
            Err(e) => registrar_erro(&format!("Falha ao contatar o GitHub: {}", e)),
        }
        return;
    }

    if args.gerar_relatorio {
        let mut sql = "SELECT projeto, fase_json, id_regra, status, desvio, sugestao FROM historico_auditoria WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();
        if let Some(ref p) = args.projeto {
            sql.push_str(" AND projeto = ?");
            params.push(p.clone());
        }
        if let Some(ref f) = args.fase {
            sql.push_str(" AND fase_json = ?");
            params.push(f.clone());
        }

        let mut stmt = conexao_db.prepare(&sql).unwrap();
        let auditorias = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .unwrap();

        let mut registros = Vec::new();
        for audit in auditorias {
            registros.push(audit.unwrap());
        }

        if registros.is_empty() {
            println!("Nenhum registro encontrado para os filtros informados.");
            return;
        }

        let mut qtd_valido = 0.0;
        let mut qtd_sim = 0.0;
        let mut qtd_nao = 0.0;
        let mut qtd_parcial = 0.0;
        let mut qtd_na = 0.0;
        let mut html_linhas = String::new();

        for reg in &registros {
            let st = reg.3.to_lowercase();
            let mut classe = "na";
            if st.contains("sim") {
                qtd_sim += 1.0;
                qtd_valido += 1.0;
                classe = "sim";
            } else if st.contains("parcialmente") {
                qtd_parcial += 1.0;
                qtd_valido += 1.0;
                classe = "parcial";
            } else if st.contains("não") || st.contains("nao") {
                if st.contains("aplica") {
                    qtd_na += 1.0;
                } else {
                    qtd_nao += 1.0;
                    qtd_valido += 1.0;
                    classe = "nao";
                }
            }
            html_linhas.push_str(&format!(
                "<tr class='{}'><td>{}</td><td>{}</td><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>\n",
                classe,
                reg.1,
                reg.2,
                reg.3,
                reg.4.trim(),
                reg.5.trim()
            ));
        }

        let indicador = if qtd_valido > 0.0 {
            (qtd_sim / qtd_valido) * 100.0
        } else {
            0.0
        };

        println!("\n====================================================================================================");
        println!("                                  RELATÓRIO DE CONFORMIDADE (VIA BANCO)");
        println!("====================================================================================================");
        println!("Aderência : {:.1}% (Indicador do Processo SpinOff)", indicador);
        println!("----------------------------------------------------------------------------------------------------");
        for reg in &registros {
            println!("[{:<13}] {}", reg.3, reg.2);
            let desvio = reg.4.trim();
            let sugestao = reg.5.trim();
            if !desvio.is_empty()
                && desvio.to_lowercase() != "nenhum"
                && desvio.to_lowercase() != "nenhum."
                && desvio.to_lowercase() != "nenhuma."
            {
                println!("  ↳ Desvio  : {}", desvio);
            }
            if !sugestao.is_empty()
                && sugestao.to_lowercase() != "nenhuma"
                && sugestao.to_lowercase() != "nenhuma."
                && sugestao.to_lowercase() != "nenhum"
            {
                println!("  ↳ Sugestão: {}", sugestao);
            }
            println!("-");
        }
        println!("====================================================================================================\n");

        if args.exportar_csv || args.html {
            let pasta_destino = args
                .pasta_saida
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap().join("relatorios"));
            if !pasta_destino.exists() {
                fs::create_dir_all(&pasta_destino).unwrap();
            }

            let proj_nome = args.projeto.unwrap_or_else(|| "Geral".to_string());

            if args.exportar_csv {
                let mut csv_content = format!(
                    "\u{FEFF}Aderência do Processo: {:.1}%\n\nProjeto;Fase;Status;Pergunta;Desvio;Sugestão\n",
                    indicador
                );
                for reg in &registros {
                    csv_content.push_str(&format!(
                        "{};{};{};{};{};{}\n",
                        reg.0,
                        reg.1,
                        reg.3,
                        reg.2.replace(';', ","),
                        reg.4.replace(';', ",").replace('\n', " "),
                        reg.5.replace(';', ",").replace('\n', " ")
                    ));
                }
                let caminho_csv = pasta_destino.join(format!("Relatorio_{}.csv", proj_nome));
                fs::write(&caminho_csv, csv_content).unwrap();
                println!(
                    "💾 CSV exportado com sucesso: {}",
                    fs::canonicalize(&caminho_csv).unwrap_or(caminho_csv).display()
                );
            }

            if args.html {
                let html_content = gerar_codigo_html(
                    &proj_nome,
                    indicador,
                    qtd_sim,
                    qtd_nao,
                    qtd_parcial,
                    qtd_na,
                    &html_linhas,
                );
                let caminho_html = pasta_destino.join(format!("Dashboard_{}.html", proj_nome));
                fs::write(&caminho_html, html_content).unwrap();
                println!(
                    "📊 Dashboard HTML exportado com sucesso: {}",
                    fs::canonicalize(&caminho_html).unwrap_or(caminho_html).display()
                );
            }
        }
        return;
    }

    let pasta_atual = std::env::current_dir().unwrap();
    let mut pasta_regras = pasta_atual.join("config_rules");

    if !pasta_regras.exists() {
        let pasta_ao_lado = caminho_executavel.join("config_rules");
        let pasta_global_linux = PathBuf::from("/etc/vio/config_rules");

        if pasta_ao_lado.exists() {
            pasta_regras = pasta_ao_lado;
        } else if pasta_global_linux.exists() {
            pasta_regras = pasta_global_linux;
        }
    }

    let jsons_para_rodar = match &args.arquivo_config {
        Some(nome) => {
            if nome.ends_with(".json") {
                vec![nome.clone()]
            } else {
                vec![format!("{}.json", nome)]
            }
        }
        None => {
            // Ordem lógica das fases do SpinOff quando todos os JSONs são executados
            let ordem_fases = [
                "iniciacao.json",
                "elaboracao.json",
                "construcao.json",
                "transicao.json",
            ];
            let mut files = Vec::new();
            for fase in &ordem_fases {
                if pasta_regras.join(fase).exists() {
                    files.push(fase.to_string());
                }
            }
            if let Ok(entradas) = fs::read_dir(&pasta_regras) {
                for entrada in entradas.flatten() {
                    if entrada.path().extension().unwrap_or_default() == "json" {
                        let nome_arq = entrada.file_name().to_string_lossy().to_string();
                        if !files.contains(&nome_arq) && nome_arq != "regras_teste.json" {
                            files.push(nome_arq);
                        }
                    }
                }
            }
            files
        }
    };

    let diretorio_alvo_projeto = Path::new(&args.caminho_projeto);
    let caminho_canonico_proj = fs::canonicalize(diretorio_alvo_projeto)
        .unwrap_or_else(|_| diretorio_alvo_projeto.to_path_buf());
    let sigla_projeto = caminho_canonico_proj
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let cliente_http = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut processo_ia_gerenciado = None;

    // Expressões regulares independentes por campo para suportar variações de grafia (ex: SUGGESTÃO) ou ordem de linhas da IA
    let re_analise = Regex::new(r"(?im)^\s*[*_#-]*\s*An[áa]lise\s*[*_]*\s*:\s*(.+)$").unwrap();
    let re_status = Regex::new(r"(?im)^\s*[*_#-]*\s*Status(?:\s+Qualitativo)?\s*[*_]*\s*:\s*[*_\[\]]*\s*(Não se aplica|Nao se aplica|N/A|NA|Sim|Não|Nao|Parcialmente)\b").unwrap();
    let re_desvio = Regex::new(r"(?im)^\s*[*_#-]*\s*Desvios?\s*[*_]*\s*:\s*(.+)$").unwrap();
    let re_sugestao = Regex::new(r"(?im)^\s*[*_#-]*\s*(?:Sugest[ãa]o|Suggest[ãa]o|Sug[eé]st\w+|Recomenda[çc][ãa]o)\s*[*_]*\s*:\s*(.+)$").unwrap();
    let re_status_pronto = Regex::new(r"(?im)Status(?:\s+Qualitativo)?\s*[*_]*\s*:\s*[*_\[\]]*\s*(?:Não se aplica|Nao se aplica|N/A|NA|Sim|Não|Nao|Parcialmente)[^\n]*\n").unwrap();

    for arquivo_json in jsons_para_rodar {
        let caminho_json = pasta_regras.join(&arquivo_json);
        if !caminho_json.exists() {
            if !is_quiet {
                println!("⚠️ Arquivo de configuração '{}' não encontrado em '{}'.", arquivo_json, pasta_regras.display());
            }
            continue;
        }
        let conteudo_json = fs::read_to_string(&caminho_json).unwrap_or_default();

        let configuracao: ConfiguracaoGlobal = match serde_json::from_str(&conteudo_json) {
            Ok(config) => config,
            Err(erro) => {
                registrar_erro(&format!("Erro ao ler JSON '{}': {}", arquivo_json, erro));
                if !is_quiet {
                    println!("⚠️ Pulando o arquivo '{}': Erro na estrutura do JSON ({}).", arquivo_json, erro);
                }
                continue;
            }
        };

        if processo_ia_gerenciado.is_none() {
            processo_ia_gerenciado = gerenciar_motor_ia(&configuracao.endpoint_slm);
        }

        let mut arquivos_conhecidos = HashSet::new();
        let mut buffer_banco_dados: Vec<(String, String, String, String, String, String, String)> = Vec::new();
        let mut identidade_projeto: String = conexao_db
            .query_row(
                "SELECT resumo_identidade FROM identidade_projeto WHERE projeto = ?1",
                [&sigla_projeto],
                |row| row.get(0),
            )
            .unwrap_or_default();

        for regra in configuracao.regras_conformidade {
            let nome_base_resolvido = regra.nome_arquivo_base.replace("{SIGLA}", &sigla_projeto);
            let pasta_alvo = resolver_pasta_alvo(diretorio_alvo_projeto, &regra.pasta_esperada);
            let apenas_existencia = regra.apenas_existencia.unwrap_or(false);

            let arquivos_encontrados = encontrar_arquivos_regra(
                &pasta_alvo,
                &nome_base_resolvido,
                &sigla_projeto,
                &regra.tipo_arquivo,
                apenas_existencia,
            );

            for arq in &arquivos_encontrados {
                arquivos_conhecidos.insert(arq.to_string_lossy().replace('\\', "/"));
            }

            if !arquivos_encontrados.is_empty() {
                if apenas_existencia {
                    let mut erro_validacao_template = String::new();
                    let mut algum_valido = false;
                    for arq in &arquivos_encontrados {
                        match extractor::validar_template_artefato(
                            arq,
                            &regra.tipo_arquivo,
                            &nome_base_resolvido,
                        ) {
                            Ok(()) => {
                                algum_valido = true;
                                break;
                            }
                            Err(e) => {
                                erro_validacao_template = e;
                            }
                        }
                    }

                    if algum_valido {
                        buffer_banco_dados.push((
                            sigla_projeto.clone(),
                            arquivo_json.clone(),
                            regra.id_regra.clone(),
                            nome_base_resolvido.clone(),
                            "Sim".to_string(),
                            "Nenhum".to_string(),
                            "Nenhuma".to_string(),
                        ));
                    } else {
                        buffer_banco_dados.push((
                            sigla_projeto.clone(),
                            arquivo_json.clone(),
                            regra.id_regra.clone(),
                            nome_base_resolvido.clone(),
                            "Não".to_string(),
                            erro_validacao_template,
                            "Recriar e preencher o artefato utilizando o template oficial atual do SpinOff.".to_string(),
                        ));
                    }
                    continue;
                }

                // Extrai e concatena o conteúdo de todos os arquivos correspondentes à regra (ex: todas as Atas ou todas as Histórias de Usuário)
                let mut textos_extraidos = Vec::new();
                let mut ultimo_erro_extracao = String::new();

                for caminho_real in &arquivos_encontrados {
                    let nome_exibicao = caminho_real
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let resultado = match regra.tipo_arquivo.to_lowercase().as_str() {
                        "excel" => extractor::extrair_excel(
                            caminho_real,
                            regra.aba_planilha.as_deref().unwrap_or(""),
                        ),
                        "word" => extractor::extrair_word(caminho_real),
                        "astah" | "uml" => extractor::extrair_astah(caminho_real),
                        _ => Err(String::from("Tipo de arquivo não suportado.")),
                    };

                    match resultado {
                        Ok(texto) => {
                            if arquivos_encontrados.len() > 1 {
                                textos_extraidos.push(format!("--- ARQUIVO: {} ---\n{}", nome_exibicao, texto));
                            } else {
                                textos_extraidos.push(texto);
                            }
                        }
                        Err(e) => {
                            ultimo_erro_extracao = e;
                        }
                    }
                }

                if !textos_extraidos.is_empty() {
                    let mut dados_brutos = textos_extraidos.join("\n\n");
                    let contexto_cruzado = obter_contexto_cruzado_comparacao(
                        diretorio_alvo_projeto,
                        &sigla_projeto,
                        &regra.id_regra,
                    );
                    if !contexto_cruzado.is_empty() {
                        dados_brutos.push_str(&contexto_cruzado);
                    }
                    // Limita tamanho máximo do conteúdo enviado por regra para evitar estouro de memória em projetos com muitos arquivos
                    if dados_brutos.len() > 14000 {
                        dados_brutos = dados_brutos.chars().take(14000).collect();
                        dados_brutos.push_str("\n...[Conteúdo truncado para caber na janela de contexto]...");
                    }

                    let mut status_extraido = String::new();
                    let mut desvio_extraido = String::new();
                    let mut sugestao_extraida = String::new();
                    let mut resposta_completa = String::new();

                    const MAX_TENTATIVAS_IA: usize = 2;
                    let mut tentativa = 0;

                    while tentativa < MAX_TENTATIVAS_IA {
                        tentativa += 1;

                        if identidade_projeto.is_empty()
                            && normalizar_texto(&regra.nome_arquivo_base).contains("visao")
                        {
                            let payload_id = PayloadOllama {
                                model: configuracao.modelo_slm.clone(),
                                prompt: format!("Resuma em uma frase o objetivo deste sistema:\n{}", dados_brutos),
                                stream: false,
                                options: OllamaOptions {
                                    num_ctx: 4096,
                                    num_predict: 60,
                                    temperature: 0.1,
                                    top_k: 10,
                                    top_p: 0.5,
                                },
                            };
                            if let Ok(res_id) = cliente_http
                                .post(&configuracao.endpoint_slm)
                                .json(&payload_id)
                                .send()
                                .await
                            {
                                if let Ok(json_id) = res_id.json::<serde_json::Value>().await {
                                    if let Some(texto_id) = json_id.get("response").and_then(|r| r.as_str()) {
                                        identidade_projeto = texto_id.trim().to_string();
                                        conexao_db
                                            .execute(
                                                "INSERT OR REPLACE INTO identidade_projeto (projeto, resumo_identidade) VALUES (?1, ?2)",
                                                (&sigla_projeto, &identidade_projeto),
                                            )
                                            .ok();
                                    }
                                }
                            }
                        }

                        let prompt_final = format!(
                            "=== CONTEÚDO EXTRAÍDO DO ARTEFATO ({}) ===\n{}\n\n=== INSTRUÇÕES DE AUDITORIA SPINOFF ===\n{}",
                            nome_base_resolvido, dados_brutos, regra.prompt_ia
                        );
                        let contexto_dinamico = ((dados_brutos.len() / 3) as u32 + 2048).clamp(4096, 8192);
                        let payload = PayloadOllama {
                            model: configuracao.modelo_slm.clone(),
                            prompt: prompt_final,
                            stream: true,
                            options: OllamaOptions {
                                num_ctx: contexto_dinamico,
                                num_predict: 320,
                                temperature: 0.1,
                                top_k: 10,
                                top_p: 0.5,
                            },
                        };

                        if let Ok(res) = cliente_http
                            .post(&configuracao.endpoint_slm)
                            .json(&payload)
                            .send()
                            .await
                        {
                            let mut stream = res.bytes_stream();
                            resposta_completa.clear();
                            let frames = ["Processando.  ", "Processando.. ", "Processando..."];
                            let mut animacao = 0;

                            while let Some(pedaco_resultado) = stream.next().await {
                                if let Ok(json_parcial) =
                                    serde_json::from_slice::<serde_json::Value>(&pedaco_resultado.unwrap_or_default())
                                {
                                    if let Some(texto) = json_parcial.get("response").and_then(|r| r.as_str()) {
                                        if is_quiet {
                                            if animacao % 5 == 0 {
                                                print!("\r{}", frames[(animacao / 5) % 3]);
                                                io::stdout().flush().unwrap();
                                            }
                                            animacao += 1;
                                        } else {
                                            print!("{}", texto);
                                            io::stdout().flush().unwrap();
                                        }
                                        resposta_completa.push_str(texto);

                                        // Encerra o stream assim que os campos obrigatórios e o Status Qualitativo tiverem sido emitidos
                                        if re_status_pronto.is_match(&resposta_completa)
                                            && re_desvio.is_match(&resposta_completa)
                                            && re_sugestao.is_match(&resposta_completa)
                                        {
                                            break;
                                        }
                                    }
                                }
                            }
                            if is_quiet {
                                print!("\r                \r");
                            } else {
                                println!("\n------------------");
                            }

                            let analise_extraida = re_analise
                                .captures(&resposta_completa)
                                .and_then(|c| c.get(1))
                                .map(|m| {
                                    m.as_str()
                                        .trim()
                                        .replace('\n', " ")
                                        .trim_matches(|c| c == '[' || c == ']' || c == '*')
                                        .trim()
                                        .to_string()
                                })
                                .unwrap_or_default();
                            desvio_extraido = re_desvio
                                .captures(&resposta_completa)
                                .and_then(|c| c.get(1))
                                .map(|m| {
                                    m.as_str()
                                        .trim()
                                        .replace('\n', " ")
                                        .trim_matches(|c| c == '[' || c == ']' || c == '*')
                                        .trim()
                                        .to_string()
                                })
                                .unwrap_or_default();
                            sugestao_extraida = re_sugestao
                                .captures(&resposta_completa)
                                .and_then(|c| c.get(1))
                                .map(|m| {
                                    m.as_str()
                                        .trim()
                                        .replace('\n', " ")
                                        .trim_matches(|c| c == '[' || c == ']' || c == '*')
                                        .trim()
                                        .to_string()
                                })
                                .unwrap_or_else(|| "Nenhuma".to_string());

                            if let Some(cap_st) = re_status.captures(&resposta_completa) {
                                status_extraido =
                                    normalizar_status_qualitativo(cap_st.get(1).unwrap().as_str());
                                // Coerência SpinOff: se a própria IA atestou "Desvio: Nenhum" com status Parcialmente, não há não-conformidade (Status = Sim)
                                if status_extraido == "Parcialmente" && eh_desvio_inexistente(&desvio_extraido) {
                                    status_extraido = "Sim".to_string();
                                }
                                if status_extraido == "Sim" || status_extraido == "Não se aplica" {
                                    desvio_extraido = "Nenhum".to_string();
                                    sugestao_extraida = "Nenhuma".to_string();
                                } else if desvio_extraido.is_empty() || eh_desvio_inexistente(&desvio_extraido) {
                                    if !analise_extraida.is_empty() {
                                        desvio_extraido = analise_extraida;
                                    } else {
                                        desvio_extraido = "Critério não atendido conforme análise do artefato.".to_string();
                                    }
                                }
                                break;
                            } else if !desvio_extraido.is_empty() && eh_desvio_inexistente(&desvio_extraido) {
                                // Se a IA declarou explicitamente "Desvio: Nenhum" e encerrou após a Sugestão sem imprimir a 4ª linha
                                status_extraido = "Sim".to_string();
                                desvio_extraido = "Nenhum".to_string();
                                sugestao_extraida = "Nenhuma".to_string();
                                break;
                            } else if tentativa == MAX_TENTATIVAS_IA {
                                if !desvio_extraido.is_empty() {
                                    status_extraido = "Parcialmente".to_string();
                                } else {
                                    status_extraido = "Parcialmente".to_string();
                                    desvio_extraido =
                                        "Resposta da IA fora do padrão estrito após 3 tentativas.".to_string();
                                    sugestao_extraida =
                                        "Revisar manualmente o item na checklist.".to_string();
                                }
                                break;
                            }
                        } else {
                            registrar_erro(&format!(
                                "Falha na comunicação HTTP com o motor Ollama ({}). Regra: {}",
                                configuracao.endpoint_slm, regra.id_regra
                            ));
                            status_extraido = "Não se aplica".to_string();
                            desvio_extraido =
                                "Motor de IA (Ollama) indisponível no momento da verificação.".to_string();
                            sugestao_extraida =
                                "Verifique se o Ollama está instalado e com o modelo phi3 baixado (ollama pull phi3)."
                                    .to_string();
                            break;
                        }
                    }

                    if !status_extraido.is_empty() {
                        buffer_banco_dados.push((
                            sigla_projeto.clone(),
                            arquivo_json.clone(),
                            regra.id_regra.clone(),
                            nome_base_resolvido.clone(),
                            status_extraido,
                            desvio_extraido,
                            sugestao_extraida,
                        ));
                    }
                } else {
                    registrar_erro(&format!(
                        "Falha ao extrair arquivo para regra '{}': {}",
                        regra.id_regra, ultimo_erro_extracao
                    ));
                    buffer_banco_dados.push((
                        sigla_projeto.clone(),
                        arquivo_json.clone(),
                        regra.id_regra.clone(),
                        nome_base_resolvido.clone(),
                        "Não".to_string(),
                        ultimo_erro_extracao,
                        "Preencher o artefato corretamente utilizando o template oficial do SpinOff.".to_string(),
                    ));
                }
            } else if regra.obrigatorio {
                buffer_banco_dados.push((
                    sigla_projeto.clone(),
                    arquivo_json.clone(),
                    regra.id_regra.clone(),
                    nome_base_resolvido.clone(),
                    "Não".to_string(),
                    format!("Artefato obrigatório ausente na pasta '{}'.", regra.pasta_esperada),
                    "Criar o artefato utilizando o template do SpinOff e salvar na pasta correta.".to_string(),
                ));
            } else {
                buffer_banco_dados.push((
                    sigla_projeto.clone(),
                    arquivo_json.clone(),
                    regra.id_regra.clone(),
                    nome_base_resolvido.clone(),
                    "Não se aplica".to_string(),
                    "Artefato opcional não presente.".to_string(),
                    "Nenhuma.".to_string(),
                ));
            }
        }

        let mut arquivos_estranhos = Vec::new();
        let pastas_ig = configuracao.pastas_ignoradas_extracao.unwrap_or_default();
        buscar_arquivos_estranhos(
            diretorio_alvo_projeto,
            &arquivos_conhecidos,
            &pastas_ig,
            &mut arquivos_estranhos,
        );

        for estranho in arquivos_estranhos {
            buffer_banco_dados.push((
                sigla_projeto.clone(),
                arquivo_json.clone(),
                format!("Arquivo não mapeado: {}", estranho),
                estranho,
                "Desconhecido".to_string(),
                "O arquivo não pertence à estrutura mapeada do projeto.".to_string(),
                "Mover para a pasta correta conforme Guia de Mapeamento SpinOff ou remover.".to_string(),
            ));
        }

        if !buffer_banco_dados.is_empty() {
            // Remove registros anteriores da mesma dupla (projeto, fase_json) para evitar duplicatas no banco
            conexao_db
                .execute(
                    "DELETE FROM historico_auditoria WHERE projeto = ?1 AND fase_json = ?2",
                    (&sigla_projeto, &arquivo_json),
                )
                .ok();

            let mut qtd_valido = 0.0;
            let mut qtd_sim = 0.0;
            let mut qtd_nao = 0.0;
            let mut qtd_parcial = 0.0;
            let mut qtd_na = 0.0;
            let mut html_linhas = String::new();

            for reg in &buffer_banco_dados {
                let st = reg.4.to_lowercase();
                let mut classe = "na";
                if st.contains("sim") {
                    qtd_sim += 1.0;
                    qtd_valido += 1.0;
                    classe = "sim";
                } else if st.contains("parcialmente") {
                    qtd_parcial += 1.0;
                    qtd_valido += 1.0;
                    classe = "parcial";
                } else if st.contains("não") || st.contains("nao") {
                    if st.contains("aplica") {
                        qtd_na += 1.0;
                    } else {
                        qtd_nao += 1.0;
                        qtd_valido += 1.0;
                        classe = "nao";
                    }
                }
                conexao_db
                    .execute(
                        "INSERT INTO historico_auditoria (projeto, fase_json, id_regra, arquivo, status, desvio, sugestao) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        (&reg.0, &reg.1, &reg.2, &reg.3, &reg.4, &reg.5, &reg.6),
                    )
                    .ok();

                html_linhas.push_str(&format!(
                    "<tr class='{}'><td>{}</td><td>{}</td><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>\n",
                    classe,
                    reg.1,
                    reg.2,
                    reg.4,
                    reg.5.trim(),
                    reg.6.trim()
                ));
            }

            let indicador = if qtd_valido > 0.0 {
                (qtd_sim / qtd_valido) * 100.0
            } else {
                0.0
            };

            println!("\n====================================================================================================");
            println!("                                          RELATÓRIO DE CONFORMIDADE ({})", arquivo_json);
            println!("====================================================================================================");
            println!("Projeto   : {}", sigla_projeto);
            println!("Aderência : {:.1}% (Indicador do Processo SpinOff)", indicador);
            println!("----------------------------------------------------------------------------------------------------");

            for reg in &buffer_banco_dados {
                println!("[{:<13}] {}", reg.4, reg.2);
                let desvio = reg.5.trim();
                let sugestao = reg.6.trim();
                if !desvio.is_empty()
                    && desvio.to_lowercase() != "nenhum"
                    && desvio.to_lowercase() != "nenhum."
                    && desvio.to_lowercase() != "nenhuma."
                {
                    println!("  ↳ Desvio  : {}", desvio);
                }
                if !sugestao.is_empty()
                    && sugestao.to_lowercase() != "nenhuma"
                    && sugestao.to_lowercase() != "nenhuma."
                    && sugestao.to_lowercase() != "nenhum"
                {
                    println!("  ↳ Sugestão: {}", sugestao);
                }
                println!("-");
            }
            println!("====================================================================================================\n");

            if args.exportar_csv || args.html {
                let pasta_destino = args
                    .pasta_saida
                    .clone()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().unwrap().join("relatorios"));
                if !pasta_destino.exists() {
                    fs::create_dir_all(&pasta_destino).unwrap();
                }

                if args.exportar_csv {
                    let mut csv_content = format!(
                        "\u{FEFF}Aderência do Processo: {:.1}%\n\nProjeto;Fase;Status;Pergunta;Desvio;Sugestão\n",
                        indicador
                    );
                    for reg in &buffer_banco_dados {
                        csv_content.push_str(&format!(
                            "{};{};{};{};{};{}\n",
                            reg.0,
                            reg.1,
                            reg.4,
                            reg.2.replace(';', ","),
                            reg.5.replace(';', ",").replace('\n', " "),
                            reg.6.replace(';', ",").replace('\n', " ")
                        ));
                    }
                    let caminho_csv = pasta_destino.join(format!(
                        "Relatorio_{}_{}.csv",
                        sigla_projeto,
                        arquivo_json.replace(".json", "")
                    ));
                    fs::write(&caminho_csv, csv_content).unwrap();
                    println!(
                        "💾 CSV exportado com sucesso: {}",
                        fs::canonicalize(&caminho_csv).unwrap_or(caminho_csv).display()
                    );
                }

                if args.html {
                    let html_content = gerar_codigo_html(
                        &sigla_projeto,
                        indicador,
                        qtd_sim,
                        qtd_nao,
                        qtd_parcial,
                        qtd_na,
                        &html_linhas,
                    );
                    let caminho_html = pasta_destino.join(format!(
                        "Dashboard_{}_{}.html",
                        sigla_projeto,
                        arquivo_json.replace(".json", "")
                    ));
                    fs::write(&caminho_html, html_content).unwrap();
                    println!(
                        "📊 Dashboard HTML exportado com sucesso: {}",
                        fs::canonicalize(&caminho_html).unwrap_or(caminho_html).display()
                    );
                }
            }
        }
    }
    if let Some(mut proc) = processo_ia_gerenciado {
        proc.kill().ok();
    }
}
