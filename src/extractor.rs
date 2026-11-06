use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use calamine::{open_workbook, DataType, Reader, Xlsx};

/// Remove acentos e coloca em minúsculas para comparação flexível de nomes de abas
fn normalizar_identificador(s: &str) -> String {
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
            '-' | '_' => ' ',
            other => other,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Converte números seriais de data do Excel (ex: 46252 -> 18/08/2026) para formato legível DD/MM/AAAA
fn formatar_data_serial_excel(valor: &str) -> String {
    if let Ok(dias) = valor.trim().parse::<i64>() {
        if (40000..=55000).contains(&dias) {
            // Base do Excel (com ajuste do bug de 1900): 30/12/1899 -> Rata Die / algoritmo civil de Howard Hinnant
            let z = dias + 693960 - 1 + 719468;
            let era = if z >= 0 { z } else { z - 146096 } / 146097;
            let doe = (z - era * 146097) as u64;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe as i64 + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let ano = if m <= 2 { y + 1 } else { y };
            return format!("{:02}/{:02}/{:04}", d, m, ano);
        }
    }
    valor.to_string()
}

/// Obtém o valor textual limpo de uma célula da linha ou string vazia se fora do índice
fn celula_em(celulas: &[String], idx: usize) -> &str {
    celulas.get(idx).map(|s| s.as_str()).unwrap_or("").trim()
}

/// Valida deterministicamente se o artefato encontrado não é um arquivo vazio/corrompido e se possui a estrutura mínima do template SpinOff
pub fn validar_template_artefato(
    caminho: &Path,
    tipo_arquivo: &str,
    nome_base: &str,
) -> Result<(), String> {
    let nome_exibicao = caminho
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let meta = fs::metadata(caminho)
        .map_err(|e| format!("Não foi possível acessar o arquivo '{}': {}", nome_exibicao, e))?;

    if !meta.is_dir() && meta.len() == 0 {
        return Err(format!(
            "O arquivo '{}' existe na pasta, mas está vazio (0 bytes) e não utiliza o template oficial do SpinOff.",
            nome_exibicao
        ));
    }

    match tipo_arquivo.to_lowercase().as_str() {
        "word" => {
            let texto = extrair_word(caminho)?;
            if texto.trim().len() < 20 {
                return Err(format!(
                    "O documento Word '{}' existe na pasta, mas está em branco (sem conteúdo do template SpinOff).",
                    nome_exibicao
                ));
            }
            Ok(())
        }
        "excel" => {
            let mut planilha: Xlsx<_> = open_workbook(caminho).map_err(|e| {
                format!(
                    "O arquivo Excel '{}' está corrompido ou em formato inválido ({}).",
                    nome_exibicao, e
                )
            })?;
            let abas_disponiveis = planilha.sheet_names().to_owned();
            let nome_norm = normalizar_identificador(nome_base);

            if nome_norm.contains("planejamento") {
                let abas_obrigatorias = [
                    "Equipe",
                    "Backlog_Produto",
                    "Planejamento",
                    "Entregas",
                    "Riscos",
                ];
                let mut ausentes = Vec::new();
                for ob in &abas_obrigatorias {
                    let ob_norm = normalizar_identificador(ob);
                    let existe = abas_disponiveis.iter().any(|d| {
                        let d_norm = normalizar_identificador(d);
                        d_norm == ob_norm || d_norm.contains(&ob_norm) || ob_norm.contains(&d_norm)
                    });
                    if !existe {
                        ausentes.push(*ob);
                    }
                }
                if !ausentes.is_empty() {
                    return Err(format!(
                        "A planilha '{}' não possui todas as abas obrigatórias do template SpinOff (abas ausentes: {}; abas encontradas: {}).",
                        nome_exibicao,
                        ausentes.join(", "),
                        abas_disponiveis.join(", ")
                    ));
                }
            } else if nome_norm.contains("roteiro de testes") {
                let tem_capa = abas_disponiveis
                    .iter()
                    .any(|d| normalizar_identificador(d).contains("capa"));
                if !tem_capa || abas_disponiveis.len() < 2 {
                    return Err(format!(
                        "A planilha '{}' não segue o template de Roteiro de Testes do SpinOff (abas encontradas: {}).",
                        nome_exibicao,
                        abas_disponiveis.join(", ")
                    ));
                }
            }

            let mut tem_alguma_celula_preenchida = false;
            for aba in &abas_disponiveis {
                if let Some(Ok(faixa)) = planilha.worksheet_range(aba) {
                    if faixa
                        .rows()
                        .any(|r| r.iter().any(|c| !matches!(c, DataType::Empty)))
                    {
                        tem_alguma_celula_preenchida = true;
                        break;
                    }
                }
            }
            if !tem_alguma_celula_preenchida {
                return Err(format!(
                    "A planilha '{}' existe na pasta, mas todas as suas abas estão completamente em branco.",
                    nome_exibicao
                ));
            }
            Ok(())
        }
        "astah" | "uml" => {
            let _ = extrair_astah(caminho)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Extrai texto limpo e estruturado de uma ou mais abas de um arquivo Excel (.xlsx)
/// Suporta múltiplas abas separadas por vírgula ou "*" / "" para ler todas as abas relevantes (ex: Roteiro de Testes)
pub fn extrair_excel(caminho: &Path, abas: &str) -> Result<String, String> {
    let mut planilha: Xlsx<_> = open_workbook(caminho)
        .map_err(|e| format!("Erro ao abrir Excel '{}': {}", caminho.display(), e))?;

    let abas_disponiveis = planilha.sheet_names().to_owned();
    let abas_trim = abas.trim();

    // Se aba_planilha for vazia ou "*", lê todas as abas úteis (ignorando abas auxiliares de cálculo interno do SpinOff)
    let abas_alvo: Vec<String> = if abas_trim.is_empty() || abas_trim == "*" {
        abas_disponiveis
            .iter()
            .filter(|nome| {
                let n = nome.trim().to_uppercase();
                n != "AUX" && !n.starts_with('#') && !n.ends_with('#')
            })
            .cloned()
            .collect()
    } else {
        let solicitadas: Vec<&str> = abas_trim
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let mut resolvidas = Vec::new();
        let mut abas_ausentes = Vec::new();
        for sol in solicitadas {
            let sol_norm = normalizar_identificador(sol);
            if let Some(real) = abas_disponiveis
                .iter()
                .find(|d| normalizar_identificador(d) == sol_norm)
            {
                resolvidas.push(real.clone());
            } else if let Some(real) = abas_disponiveis.iter().find(|d| {
                let d_norm = normalizar_identificador(d);
                sol_norm.len() >= 4
                    && d_norm.len() >= 4
                    && (d_norm.contains(&sol_norm) || sol_norm.contains(&d_norm))
            }) {
                resolvidas.push(real.clone());
            } else {
                abas_ausentes.push(sol.to_string());
            }
        }
        if !abas_ausentes.is_empty() {
            return Err(format!(
                "A(s) aba(s) obrigatória(s) '{}' não foi(ram) encontrada(s) na planilha (abas presentes no arquivo: {}).",
                abas_ausentes.join(", "),
                if abas_disponiveis.is_empty() {
                    "nenhuma".to_string()
                } else {
                    abas_disponiveis.join(", ")
                }
            ));
        }
        resolvidas
    };

    let mut dados_extraidos = String::new();
    let mut encontrou_alguma_aba = false;
    let mut total_registros_uteis = 0usize;

    for aba in &abas_alvo {
        if let Some(Ok(faixa)) = planilha.worksheet_range(aba) {
            encontrou_alguma_aba = true;
            let aba_norm = normalizar_identificador(aba);
            dados_extraidos.push_str(&format!("\n--- CONTEÚDO DA ABA: {} ---\n", aba));

            let linhas_matriz: Vec<Vec<String>> = faixa
                .rows()
                .map(|linha| {
                    linha
                        .iter()
                        .map(|celula| match celula {
                            DataType::String(s) => s.trim().replace('\n', " ").replace('\r', ""),
                            DataType::Float(f) => {
                                if (f.fract()).abs() < 1e-6 {
                                    format!("{:.0}", f)
                                } else {
                                    format!("{:.2}", f)
                                }
                            }
                            DataType::Int(i) => i.to_string(),
                            DataType::Bool(b) => b.to_string(),
                            DataType::DateTime(dt) => dt.to_string(),
                            DataType::DateTimeIso(s) | DataType::DurationIso(s) => {
                                s.trim().to_string()
                            }
                            _ => String::new(),
                        })
                        .collect()
                })
                .collect();

            if aba_norm.contains("backlog") {
                // No template SpinOff, Backlog_Produto possui DUAS tabelas lado a lado nas mesmas linhas:
                // Colunas A..D (0..=3): Atividades por Fase e Estimativa (Horas)
                // Colunas F..L (5..=11): Casos de Uso / Histórias de Usuário e Priorização VRDC
                let mut qtd_atividades = 0usize;
                let mut qtd_ucs = 0usize;

                dados_extraidos.push_str("[TABELA 1: ATIVIDADES E ESTIMATIVAS DE ESFORÇO DO BACKLOG (Colunas A a D)]\n");
                for (idx, linha) in linhas_matriz.iter().enumerate() {
                    if idx < 4 {
                        continue;
                    }
                    let id = celula_em(linha, 0);
                    let fase = celula_em(linha, 1);
                    let tarefa = celula_em(linha, 2);
                    let horas = celula_em(linha, 3);
                    if !fase.is_empty() && !tarefa.is_empty() && tarefa != "-" {
                        qtd_atividades += 1;
                        dados_extraidos.push_str(&format!(
                            "- Atividade {}: '{}' (Fase: {}, Estimativa de Esforço: {} horas)\n",
                            id, tarefa, fase, horas
                        ));
                    }
                }
                if qtd_atividades == 0 {
                    dados_extraidos.push_str("- [DESVIO CRÍTICO: TABELA 1 VAZIA — Nenhuma atividade do projeto nas fases Iniciação/Elaboração/Construção/Transição e nenhuma estimativa de esforço em horas foram preenchidas!]\n");
                }

                dados_extraidos.push_str("\n[TABELA 2: CASOS DE USO DO PRODUTO E PRIORIZAÇÃO VRDC (Colunas F a L)]\n");
                for (idx, linha) in linhas_matriz.iter().enumerate() {
                    if idx < 4 {
                        continue;
                    }
                    let seq = celula_em(linha, 5);
                    let uc = celula_em(linha, 6);
                    let valor = celula_em(linha, 7);
                    let risco = celula_em(linha, 8);
                    let dep = celula_em(linha, 9);
                    let comp = celula_em(linha, 10);
                    let prio = celula_em(linha, 11);
                    if !uc.is_empty() && uc != "-" {
                        qtd_ucs += 1;
                        dados_extraidos.push_str(&format!(
                            "- Caso de Uso {}: '{}' (Valor V={}, Risco R={}, Dependência D={}, Complexidade Técnica C={}, Prioridade Total={})\n",
                            seq, uc, valor, risco, dep, comp, prio
                        ));
                    }
                }
                if qtd_ucs == 0 {
                    dados_extraidos.push_str("- [DESVIO CRÍTICO: TABELA 2 VAZIA — Nenhum Caso de Uso do produto e nenhuma priorização VRDC foram preenchidos!]\n");
                }

                if qtd_atividades == 0 && qtd_ucs == 0 {
                    return Err(
                        "A aba 'Backlog_Produto' está completamente vazia (nenhuma atividade, estimativa de horas ou caso de uso foi preenchido)."
                            .to_string(),
                    );
                }
                total_registros_uteis += qtd_atividades + qtd_ucs;
            } else if aba_norm.contains("risco") {
                // Na aba Riscos do SpinOff, as linhas superiores são listas suspensas de apoio; os riscos reais ficam abaixo de "Riscos do Projeto"
                dados_extraidos.push_str("[LISTA DE RISCOS DO PROJETO E PLANOS DE AÇÃO]\n");
                let mut em_tabela_riscos = false;
                let mut qtd_riscos = 0usize;
                for linha in &linhas_matriz {
                    let col_a = celula_em(linha, 0);
                    let col_b = celula_em(linha, 1);
                    if col_a.to_lowercase().contains("riscos do projeto")
                        || col_b.to_lowercase().contains("descri")
                    {
                        em_tabela_riscos = true;
                        continue;
                    }
                    if em_tabela_riscos && !col_b.is_empty() {
                        qtd_riscos += 1;
                        let data = formatar_data_serial_excel(celula_em(linha, 2));
                        let prob = celula_em(linha, 3);
                        let imp = celula_em(linha, 4);
                        let perda = celula_em(linha, 5);
                        let sit = celula_em(linha, 6);
                        let elim = celula_em(linha, 7);
                        let mitig = celula_em(linha, 9);
                        let conting = celula_em(linha, 10);
                        dados_extraidos.push_str(&format!(
                            "- Risco {}: '{}' (Identificado em: {}, Probabilidade: {}, Impacto: {}, Perda Esperada: {}, Situação: {}, Plano de Eliminação: {}, Plano de Mitigação: {}, Plano de Contingência: {})\n",
                            col_a, col_b, data, prob, imp, perda, sit, elim, mitig, conting
                        ));
                    }
                }
                if qtd_riscos == 0 {
                    return Err(
                        "A aba 'Riscos' está vazia (nenhum risco do projeto ou plano de ação foi preenchido)."
                            .to_string(),
                    );
                }
                total_registros_uteis += qtd_riscos;
            } else if aba_norm.contains("entrega") {
                dados_extraidos.push_str("[PLANO DE ENTREGAS POR SPRINT (PREVISTO X REALIZADO)]\n");
                let mut qtd_entregas = 0usize;
                for (idx, linha) in linhas_matriz.iter().enumerate() {
                    if idx < 4 {
                        continue;
                    }
                    let sprint = celula_em(linha, 0);
                    let produtos = celula_em(linha, 1);
                    if !sprint.is_empty() && !produtos.is_empty() {
                        qtd_entregas += 1;
                        let ini_prev = formatar_data_serial_excel(celula_em(linha, 2));
                        let fim_prev = formatar_data_serial_excel(celula_em(linha, 3));
                        let hs_prev = celula_em(linha, 4);
                        let status = celula_em(linha, 5);
                        let ini_real = formatar_data_serial_excel(celula_em(linha, 6));
                        let fim_real = formatar_data_serial_excel(celula_em(linha, 7));
                        let hs_real = celula_em(linha, 8);
                        let versao = celula_em(linha, 10);
                        dados_extraidos.push_str(&format!(
                            "- Sprint {}: Entregas='{}', Período Previsto={} a {} ({}h), Status={}, Período Realizado={} a {} ({}h), Versão={}\n",
                            sprint, produtos, ini_prev, fim_prev, hs_prev, status, ini_real, fim_real, hs_real, versao
                        ));
                    }
                }
                if qtd_entregas == 0 {
                    return Err(
                        "A aba 'Entregas' está vazia (nenhum sprint ou plano de entregas foi preenchido)."
                            .to_string(),
                    );
                }
                total_registros_uteis += qtd_entregas;
            } else if aba_norm.contains("equipe") {
                dados_extraidos.push_str("[MEMBROS DA EQUIPE DO PROJETO]\n");
                let mut qtd_membros = 0usize;
                for (idx, linha) in linhas_matriz.iter().enumerate() {
                    if idx < 3 {
                        continue;
                    }
                    let papel = celula_em(linha, 0);
                    let nome = celula_em(linha, 1);
                    let email = celula_em(linha, 2);
                    if !nome.is_empty() || !email.is_empty() {
                        qtd_membros += 1;
                        dados_extraidos.push_str(&format!(
                            "- Integrante: {} (Papel: {}, E-mail: {})\n",
                            nome, papel, email
                        ));
                    }
                }
                if qtd_membros == 0 {
                    return Err(
                        "A aba 'Equipe' está vazia (nenhum integrante, papel ou e-mail foi preenchido)."
                            .to_string(),
                    );
                }
                total_registros_uteis += qtd_membros;
            } else {
                // Abas gerais (ex: Planejamento, Capa e abas de Casos de Uso do Roteiro de Testes)
                let mut em_cenarios_teste = false;
                let mut linhas_aba = 0usize;
                for linha in &linhas_matriz {
                    let col_a = celula_em(linha, 0);
                    let col_b = celula_em(linha, 1);
                    if col_a.to_lowercase().contains("cenário") || col_a.to_lowercase().contains("cenario") {
                        em_cenarios_teste = true;
                    }

                    if em_cenarios_teste {
                        // Nas abas de casos de teste do Roteiro de Testes, ignora linhas numeradas vazias do template
                        if col_a.parse::<u32>().is_ok() {
                            if col_b.is_empty() {
                                continue;
                            }
                            linhas_aba += 1;
                            let entradas = celula_em(linha, 2);
                            let esperado = celula_em(linha, 3);
                            let obs = celula_em(linha, 4);
                            let defeitos = celula_em(linha, 5);
                            let ciclos = celula_em(linha, 6);
                            dados_extraidos.push_str(&format!(
                                "- Cenário {}: '{}' (Dados de Entrada: '{}', Resultado Esperado: '{}', Observações/Resultado da Execução: '{}', Qde. Defeitos: '{}', Qde. Ciclos Executados: '{}')\n",
                                col_a, col_b, entradas, esperado, obs, defeitos, ciclos
                            ));
                            continue;
                        }
                    }

                    let mut celulas_limpas: Vec<String> = linha
                        .iter()
                        .map(|c| formatar_data_serial_excel(c))
                        .collect();
                    while celulas_limpas.last().map(|s| s.is_empty()).unwrap_or(false) {
                        celulas_limpas.pop();
                    }
                    let nao_vazias: Vec<&str> = celulas_limpas
                        .iter()
                        .map(|s| s.as_str())
                        .filter(|s| !s.is_empty() && !s.starts_with('#'))
                        .collect();

                    // Ignora linhas completamente vazias ou com apenas um número isolado de template
                    if nao_vazias.len() == 1 && nao_vazias[0].parse::<f64>().is_ok() {
                        continue;
                    }
                    if !nao_vazias.is_empty() {
                        linhas_aba += 1;
                        dados_extraidos.push_str(&nao_vazias.join(" | "));
                        dados_extraidos.push('\n');
                    }
                }
                if abas_alvo.len() == 1 && linhas_aba == 0 {
                    return Err(format!("A aba '{}' está vazia (nenhum dado preenchido).", aba));
                }
                total_registros_uteis += linhas_aba;
            }
        } else {
            return Err(format!(
                "A aba obrigatória '{}' não foi encontrada na planilha (abas disponíveis: {}).",
                aba,
                abas_disponiveis.join(", ")
            ));
        }
    }

    if !encontrou_alguma_aba || total_registros_uteis == 0 {
        return Err(format!(
            "Nenhum dado preenchido foi encontrado nas abas '{}' da planilha '{}'.",
            abas_trim,
            caminho.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    Ok(dados_extraidos)
}

/// Decodifica entidades XML básicas presentes no document.xml do Word
fn decodificar_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Remove blocos longos de instruções do template SpinOff entre colchetes [...] mantendo o documento enxuto
fn remover_instrucoes_colchetes(texto: &str) -> String {
    let mut resultado = String::with_capacity(texto.len());
    let mut dentro_colchetes = 0usize;
    for c in texto.chars() {
        if c == '[' {
            dentro_colchetes += 1;
        } else if c == ']' && dentro_colchetes > 0 {
            dentro_colchetes -= 1;
        } else if dentro_colchetes == 0 {
            resultado.push(c);
        }
    }
    resultado
}

/// Abre o ZIP oculto do Word (.docx) e extrai o texto preservando quebras de parágrafo e tabelas
pub fn extrair_word(caminho: &Path) -> Result<String, String> {
    let nome_exibicao = caminho
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if let Ok(meta) = fs::metadata(caminho) {
        if meta.len() == 0 {
            return Err(format!(
                "O documento Word '{}' está vazio (0 bytes).",
                nome_exibicao
            ));
        }
    }

    let arquivo = fs::File::open(caminho)
        .map_err(|e| format!("Erro ao abrir arquivo DOCX '{}': {}", nome_exibicao, e))?;

    let mut archive = zip::ZipArchive::new(arquivo).map_err(|_| {
        format!(
            "O arquivo '{}' está corrompido ou não é um documento Word (.docx) válido.",
            nome_exibicao
        )
    })?;

    let mut xml = archive.by_name("word/document.xml").map_err(|_| {
        format!(
            "Estrutura interna document.xml ausente no arquivo '{}'.",
            nome_exibicao
        )
    })?;

    let mut conteudo = String::new();
    xml.read_to_string(&mut conteudo)
        .map_err(|e| format!("Erro ao ler XML do DOCX '{}': {}", nome_exibicao, e))?;

    // Preserva estrutura de tabelas (</w:tc> -> " | ", </w:tr> -> "\n") e parágrafos (</w:p> -> "\n")
    let estruturado = conteudo
        .replace("</w:tc>", "<w:t> | </w:t>")
        .replace("</w:tr>", "<w:t>\n</w:t>")
        .replace("</w:p>", "<w:t>\n</w:t>")
        .replace("<w:br/>", "<w:t>\n</w:t>")
        .replace("<w:tab/>", "<w:t>\t</w:t>");

    let mut texto_bruto = String::new();
    for pedaco in estruturado.split("<w:t").skip(1) {
        if let Some(fim_tag) = pedaco.find('>') {
            if let Some(inicio_fechamento) = pedaco.find("</w:t>") {
                let trecho = decodificar_xml(&pedaco[fim_tag + 1..inicio_fechamento]);
                texto_bruto.push_str(&trecho);
            }
        }
    }

    let texto_sem_instrucoes = remover_instrucoes_colchetes(&texto_bruto);

    // Limpa linhas vazias repetidas e espaços supérfluos mantendo a legibilidade das tabelas
    let mut linhas_limpas = Vec::new();
    for linha in texto_sem_instrucoes.lines() {
        let l = linha.trim().trim_matches('|').trim();
        if !l.is_empty() && l != "|" {
            linhas_limpas.push(l.to_string());
        }
    }

    let texto_final = linhas_limpas.join("\n");
    if texto_final.trim().len() < 20 {
        return Err(format!(
            "O documento Word '{}' está em branco (sem conteúdo preenchido).",
            nome_exibicao
        ));
    }

    Ok(texto_final)
}

/// Verifica se uma string do EntityStore do Astah é um identificador interno/UUID
fn eh_uuid_astah(s: &str) -> bool {
    if s.len() < 18 {
        return false;
    }
    if let Some(pos) = s.find('-') {
        if (1..=10).contains(&pos) {
            let resto = &s[pos + 1..];
            return resto.len() >= 16
                && resto
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c.is_ascii_lowercase() || c == '-');
        }
    }
    false
}

/// Verifica se uma string extraída do Astah é metadado interno da ferramenta ou classe padrão do Java
fn eh_metadado_interno_astah(s: &str) -> bool {
    let prefixos_ignorados = [
        "JP.co.", "LJP/", "Ljava", "[L", "jude.", "org.omg", "java.", "javax.",
        "uml.", "er.", "dfd.", "sysml.", "view.", "diagram.", "other.", "shadow.",
        "fill.", "line.", "isfilled.", "attribute.", "operation.", "association.",
        "protected.", "private.", "public.", "package.", "visibility.", "<?xml",
        "Community ", "community ", "name=", "target=", "visibility=", "required=",
        "tags=", "label=", "stereotype=", "defaultvalue=", "definition=", "type=",
        "omit=", "class,attribute", "annotations,",
    ];
    if prefixos_ignorados.iter().any(|p| s.starts_with(p)) {
        return true;
    }
    if s.starts_with('#') && s.len() == 7 && s[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    let palavras_internas = [
        "A.C", "ProjectModel", "Cloneable", "Comparable", "Boolean", "Byte", "Character",
        "Class", "Double", "Float", "Integer", "Long", "Math", "Number", "Object", "Short",
        "String", "StringBuffer", "System", "Thread", "Throwable", "Void", "Runnable",
        "Exception", "Iterable", "Appendable", "Readable", "StringBuilder", "CharSequence",
        "AbstractStringBuilder", "ProcessBuilder", "List", "ArrayList", "Arrays", "BitSet",
        "Calendar", "Collection", "Collections", "Comparator", "Currency", "Date",
        "Dictionary", "Enumeration", "EventListener", "EventListenerProxy", "EventObject",
        "HashMap", "HashSet", "Hashtable", "IdentityHashMap", "Iterator", "LinkedHashMap",
        "LinkedHashSet", "LinkedList", "ListIterator", "Map", "Observable", "Observer",
        "Set", "SortedMap", "SortedSet", "Stack", "StringTokenizer", "TreeMap", "TreeSet",
        "Vector", "WeakHashMap", "RandomAccess", "BlockingQueue", "Queue", "Formattable",
        "AbstractQueue", "EnumMap", "EnumSet", "FormattableFlags", "Formatter",
        "PriorityQueue", "Scanner", "UUID", "BlockingDeque", "Deque", "NavigableSet",
        "ArrayDeque", "ConcurrentSkipListMap", "ConcurrentSkipListSet", "ServiceLoader",
        "Optional", "OptionalDouble", "OptionalInt", "OptionalLong", "Resource", "Event",
        "Summary", "interface", "none", "public", "private", "protected", "package",
        "instance", "abstract", "final", "static", "synchronized", "native", "transient",
        "volatile", "strictfp", "true", "false", "null", "int", "long", "double", "float",
        "boolean", "char", "byte", "short", "void", "return", "sequential", "composite",
        "aggregate", "navigable", "unspecified navigable", "line", "line_right_angle",
        "boundary", "control", "entity", "actor", "extend", "include", "extends", "bind",
        "actual_value", "java", "lang", "util", "shape", "border_rect", "0..*", "0..1",
        "1..*", "Enum", "enum", "Model",
    ];
    palavras_internas.contains(&s)
}

fn eh_nome_diagrama_ou_pacote_modelo(nome: &str) -> bool {
    let n = normalizar_identificador(nome);
    n.contains("diagram")
        || n.contains("modelo")
        || n.contains("arquitetura")
        || n.contains("deployment")
        || n.contains("implantacao")
        || n == "1 analise"
        || n == "2 design"
        || n == "3 arquitetura"
        || n == "4 deployment"
}

/// Extrai elementos UML, diagramas, atores, casos de uso, classes e descrições de arquivos .asta (Astah) ou .uml textuais
pub fn extrair_astah(caminho: &Path) -> Result<String, String> {
    let nome_exibicao = caminho
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if let Ok(meta) = fs::metadata(caminho) {
        if meta.len() == 0 {
            return Err(format!(
                "O arquivo de modelagem UML '{}' está vazio (0 bytes).",
                nome_exibicao
            ));
        }
    }

    // 1. Se for um arquivo UML em texto puro (.uml, .puml, .xmi), lê diretamente
    if let Ok(texto_puro) = fs::read_to_string(caminho) {
        if !texto_puro.trim().is_empty() {
            return Ok(texto_puro);
        }
    }

    // 2. Arquivos .asta do Astah são pacotes ZIP contendo o fluxo Java Serialization "EntityStore"
    let arquivo = fs::File::open(caminho)
        .map_err(|e| format!("Erro ao abrir arquivo Astah '{}': {}", nome_exibicao, e))?;

    let mut archive = zip::ZipArchive::new(arquivo).map_err(|_| {
        format!(
            "O arquivo '{}' está corrompido ou não é um arquivo Astah (.asta) válido.",
            nome_exibicao
        )
    })?;

    let mut bytes = Vec::new();
    if !archive.is_empty() {
        if let Ok(mut entry) = archive.by_index(0) {
            let _ = entry.read_to_end(&mut bytes);
        }
    }

    if bytes.len() < 4 {
        return Err(format!(
            "O arquivo Astah '{}' está vazio ou corrompido.",
            nome_exibicao
        ));
    }

    // Extrai todas as strings TC_STRING (0x74 + u16 big-endian length + UTF-8) do fluxo Java Serialization
    let mut strings_brutas: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0x74 {
            let len = ((bytes[i + 1] as usize) << 8) | (bytes[i + 2] as usize);
            if (2..=2000).contains(&len) && i + 3 + len <= bytes.len() {
                let fatia = &bytes[i + 3..i + 3 + len];
                let bytes_validos = fatia
                    .iter()
                    .all(|&b| b >= 0x20 || b == b'\n' || b == b'\r' || b == b'\t');
                if bytes_validos {
                    if let Ok(texto) = std::str::from_utf8(fatia) {
                        let limpo = texto.trim();
                        if !limpo.is_empty() {
                            strings_brutas.push(limpo.to_string());
                        }
                        i += 3 + len;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }

    // No UModelElementImp do Astah, um elemento possui [UUID] -> [Descrição opcional] -> [Nome do Elemento]
    let mut elementos_com_descricao = Vec::new();
    let mut outros_elementos = Vec::new();
    let mut diagramas_encontrados = Vec::new();
    let mut vistos = HashSet::new();

    let mut j = 1;
    while j < strings_brutas.len() {
        let atual = &strings_brutas[j];
        let anterior = &strings_brutas[j - 1];

        if !eh_uuid_astah(atual) && !eh_metadado_interno_astah(atual) && eh_uuid_astah(anterior) {
            let proximo = strings_brutas.get(j + 1).map(|s| s.as_str()).unwrap_or("");
            if !proximo.is_empty() && !eh_uuid_astah(proximo) && !eh_metadado_interno_astah(proximo) {
                let nome = proximo.split_whitespace().collect::<Vec<_>>().join(" ");
                let desc = atual.split_whitespace().collect::<Vec<_>>().join(" ");
                let chave = format!("{}|{}", nome, desc);
                if vistos.insert(chave) {
                    if eh_nome_diagrama_ou_pacote_modelo(&nome) {
                        diagramas_encontrados.push(format!("- Diagrama/Modelo: {} | Descrição: {}", nome, desc));
                    } else {
                        elementos_com_descricao.push(format!("- Elemento: '{}' | Descrição UML: '{}'", nome, desc));
                    }
                }
                j += 2;
                continue;
            } else {
                let texto_limpo = atual.split_whitespace().collect::<Vec<_>>().join(" ");
                if vistos.insert(texto_limpo.clone()) {
                    // Quando o nome do elemento já foi serializado antes (ex: UC05 - Postar como pacote e como caso de uso),
                    // o Java Serialization usa TC_REFERENCE para o nome e apenas a frase de Descrição aparece como TC_STRING.
                    let parece_descricao = texto_limpo.len() > 45
                        || texto_limpo.ends_with('.')
                        || texto_limpo.to_lowercase().starts_with("permite ");
                    if parece_descricao {
                        elementos_com_descricao.push(format!(
                            "- Descrição UML de Caso de Uso/Elemento: '{}'",
                            texto_limpo
                        ));
                    } else if eh_nome_diagrama_ou_pacote_modelo(&texto_limpo) {
                        diagramas_encontrados.push(format!("- Diagrama/Modelo: {}", texto_limpo));
                    } else {
                        outros_elementos.push(format!("- {}", texto_limpo));
                    }
                }
            }
        }
        j += 1;
    }

    let mut resumo = String::new();
    resumo.push_str(&format!(
        "=== MODELO UML ASTAH EXTRAÍDO: {} ===\n",
        caminho.file_name().unwrap_or_default().to_string_lossy()
    ));

    if !diagramas_encontrados.is_empty() {
        resumo.push_str("\n[DIAGRAMAS E MODELOS IDENTIFICADOS NO ARQUIVO ASTAH]:\n");
        for d in &diagramas_encontrados {
            resumo.push_str(d);
            resumo.push('\n');
        }
    }

    if !elementos_com_descricao.is_empty() {
        resumo.push_str("\n[CASOS DE USO E ELEMENTOS COM DESCRIÇÃO UML PREENCHIDA]:\n");
        for e in &elementos_com_descricao {
            resumo.push_str(e);
            resumo.push('\n');
        }
    }

    if !outros_elementos.is_empty() {
        resumo.push_str("\n[ATORES, CLASSES, PACOTES, ATRIBUTOS E NÓS DE ARQUITETURA/DEPLOYMENT]:\n");
        for e in &outros_elementos {
            resumo.push_str(e);
            resumo.push('\n');
        }
    }

    Ok(resumo)
}
