import { useEffect, useRef, useState } from "react";
import { aiCancelJob, aiCreateJob, aiGetJob } from "../api";
import type {
  AiJob,
  AiSolutionWorkflowResult,
  ProblemFull,
  ProblemSolutionVariant,
  SolutionBlock,
  SolutionPlan,
  SolutionStrategy,
  StrategyValidationResult,
  VerificationResult,
} from "../types";
import type { SolutionSubject } from "./AiConvertDialog";
import { useApp } from "../store";
import { Icon } from "./Icon";
import { Modal } from "./ui";

const RUNNING = new Set(["queued", "preprocessing", "waiting_for_codex", "converting", "validating", "compiling"]);

const DIFFICULTY_LABEL: Record<string, string> = {
  basic: "基礎",
  standard: "標準",
  advanced: "発展",
};

const LENGTH_LABEL: Record<string, string> = {
  short: "短い",
  medium: "中程度",
  long: "長い",
};

function workflowResult(job: AiJob): AiSolutionWorkflowResult {
  const result = job.structuredResult;
  if (!result || !("kind" in result)) {
    throw new Error("AIから構造化された結果を取得できませんでした");
  }
  return result as AiSolutionWorkflowResult;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function blocksFromSolution(solution: string): SolutionBlock[] {
  const chunks = solution
    .split(/\n\s*\n/)
    .map((content) => content.trim())
    .filter(Boolean);
  return chunks.map((content, index) => ({
    id: `solution-step-${index + 1}`,
    content,
    role: index === 0 ? "setup" : index === chunks.length - 1 ? "conclusion" : "reasoning",
  }));
}

function orderedVariants(variants: ProblemSolutionVariant[]): ProblemSolutionVariant[] {
  return [...variants].sort((a, b) => (a.role === b.role ? 0 : a.role === "main" ? -1 : 1));
}

function composeVariantSolutions(variants: ProblemSolutionVariant[]): string {
  return orderedVariants(variants)
    .filter((variant) => variant.solution.trim())
    .map((variant, index) => {
      if (index === 0) return variant.solution.trim();
      return `【別解${index}】\n${variant.solution.trim()}`;
    })
    .join("\n\n");
}

function composeVariantExplanations(variants: ProblemSolutionVariant[]): string {
  return orderedVariants(variants)
    .filter((variant) => variant.explanation?.trim())
    .map((variant, index) => {
      if (index === 0) return variant.explanation!.trim();
      return `【別解${index}の解説】\n${variant.explanation!.trim()}`;
    })
    .join("\n\n");
}

function legacyVariant(problem: ProblemFull): ProblemSolutionVariant | null {
  if (!problem.answer_latex.trim() && !problem.explanation_latex.trim()) return null;
  return {
    id: "variant-legacy",
    strategy: {
      id: "strategy-legacy-default",
      title: "既存の解答（legacy/default）",
      summary: "Strategy情報がない既存問題の解答を、そのまま主解法として扱います。",
      difficulty: "standard",
      answerLength: "medium",
      concepts: [],
      suitability: { examAnswer: true, textbookExplanation: true, alternativeSolution: false },
      note: "既存データとの互換用",
    },
    role: "main",
    solution: problem.answer_latex,
    solutionBlocks: blocksFromSolution(problem.answer_latex),
    explanation: problem.explanation_latex || undefined,
    explanationSections: problem.explanation_latex
      ? [{ solutionBlockIds: blocksFromSolution(problem.answer_latex).map((block) => block.id), content: problem.explanation_latex }]
      : [],
    explanationOutdated: false,
  };
}

function splitVariantText(text: string, explanation: boolean): string[] {
  const marker = explanation
    ? /(?:^|\r?\n)\s*【別解\d+の解説】\s*(?:\r?\n|$)/
    : /(?:^|\r?\n)\s*【別解\d+】\s*(?:\r?\n|$)/;
  return text.split(marker).map((part) => part.trim()).filter(Boolean);
}

/** 従来の統合欄を手編集した場合も、そこに表示中の最新版をAI入力の正本にする。 */
function initialVariantsForProblem(problem: ProblemFull): ProblemSolutionVariant[] {
  if (problem.solution_variants.length === 0) {
    return [legacyVariant(problem)].filter((variant): variant is ProblemSolutionVariant => variant !== null);
  }
  let reconciled = orderedVariants(problem.solution_variants).map((variant) => ({ ...variant }));
  if (composeVariantSolutions(reconciled).trim() !== problem.answer_latex.trim()) {
    const parts = splitVariantText(problem.answer_latex, false);
    if (parts.length === reconciled.length) {
      reconciled = reconciled.map((variant, index) => ({
        ...variant,
        solution: parts[index],
        solutionBlocks: blocksFromSolution(parts[index]),
        verification: undefined,
        explanationOutdated: Boolean(variant.explanation),
      }));
    } else {
      const previousMain = reconciled.find((variant) => variant.role === "main") ?? reconciled[0];
      reconciled = [{
        ...previousMain,
        id: `${previousMain.id}-edited`,
        strategy: {
          id: "strategy-current-edited-answer",
          title: "現在の手動編集済み解答",
          summary: "問題編集画面で編集された統合答案を、そのまま確定済みの主解答として扱います。",
          difficulty: "standard",
          answerLength: "medium",
          concepts: [],
          suitability: { examAnswer: true, textbookExplanation: true, alternativeSolution: false },
          note: "最新版解答を優先",
        },
        role: "main",
        plan: undefined,
        solution: problem.answer_latex,
        solutionBlocks: blocksFromSolution(problem.answer_latex),
        verification: undefined,
        explanationOutdated: Boolean(problem.explanation_latex),
      }];
    }
  }
  if (composeVariantExplanations(reconciled).trim() !== problem.explanation_latex.trim()) {
    const parts = splitVariantText(problem.explanation_latex, true);
    if (parts.length === reconciled.length) {
      reconciled = reconciled.map((variant, index) => ({
        ...variant,
        explanation: parts[index],
        explanationOutdated: false,
        explanationSections: [{
          solutionBlockIds: (variant.solutionBlocks ?? []).map((block) => block.id),
          content: parts[index],
        }],
      }));
    } else if (reconciled.length > 0) {
      reconciled = reconciled.map((variant, index) => ({
        ...variant,
        explanation: index === 0 && problem.explanation_latex.trim()
          ? problem.explanation_latex
          : undefined,
        explanationOutdated: false,
        explanationSections: index === 0 && problem.explanation_latex.trim()
          ? [{ solutionBlockIds: (variant.solutionBlocks ?? []).map((block) => block.id), content: problem.explanation_latex }]
          : [],
      }));
    }
  }
  return reconciled;
}

function fallbackCustomStrategy(text: string): SolutionStrategy {
  const summary = text.trim().slice(0, 900);
  return {
    id: `strategy-custom-${Date.now()}`,
    title: "ユーザー指定の解法",
    summary,
    difficulty: "standard",
    answerLength: "medium",
    concepts: [],
    suitability: { examAnswer: true, textbookExplanation: true, alternativeSolution: true },
    note: "未検証の方針をそのまま試します",
  };
}

function strategyFeatures(strategy: SolutionStrategy): string[] {
  const features: string[] = [];
  if (strategy.suitability?.examAnswer) features.push("答案向き");
  if (strategy.suitability?.textbookExplanation) features.push("解説向き");
  if (strategy.suitability?.alternativeSolution) features.push("別解向き");
  if (strategy.answerLength === "short") features.push("最短候補");
  if (strategy.note) features.push(strategy.note);
  return features;
}

function StrategyCard({
  strategy,
  selected,
  role,
  onToggle,
  onRole,
}: {
  strategy: SolutionStrategy;
  selected: boolean;
  role: "main" | "alternative";
  onToggle: () => void;
  onRole: (role: "main" | "alternative") => void;
}) {
  return (
    <article
      className="card p-3"
      style={selected ? { borderColor: "var(--accent)", background: "var(--accent-dim)" } : undefined}
    >
      <div className="flex items-start gap-2">
        <input type="checkbox" checked={selected} onChange={onToggle} className="mt-1" />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-bold">{strategy.title}</h3>
          <div className="mt-1 flex flex-wrap gap-1">
            {strategy.difficulty && <span className="badge badge-muted">{DIFFICULTY_LABEL[strategy.difficulty]}</span>}
            {strategy.answerLength && <span className="badge badge-muted">{LENGTH_LABEL[strategy.answerLength]}</span>}
            {strategyFeatures(strategy).slice(0, 3).map((feature) => (
              <span key={feature} className="badge badge-muted">{feature}</span>
            ))}
          </div>
          <p className="mt-2 text-xs leading-5 whitespace-pre-wrap">{strategy.summary}</p>
          {!!strategy.concepts?.length && (
            <p className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>
              主要知識: {strategy.concepts.join(" / ")}
            </p>
          )}
        </div>
      </div>
      {selected && (
        <div className="mt-2 flex justify-end gap-1">
          <button type="button" className={`btn btn-sm ${role === "main" ? "btn-outline" : "btn-ghost"}`} onClick={() => onRole("main")}>主解法として使用</button>
          <button type="button" className={`btn btn-sm ${role === "alternative" ? "btn-outline" : "btn-ghost"}`} onClick={() => onRole("alternative")}>別解として使用</button>
        </div>
      )}
    </article>
  );
}

export function AiSolutionWorkflowDialog({
  problem,
  solutionSubject,
  onChange,
  onClose,
}: {
  problem: ProblemFull;
  solutionSubject: SolutionSubject;
  onChange: (variants: ProblemSolutionVariant[], answer: string, explanation: string) => void;
  onClose: () => void;
}) {
  const { showToast } = useApp();
  const initialVariants = initialVariantsForProblem(problem);
  const [screen, setScreen] = useState<"start" | "candidates" | "custom" | "variants">(
    initialVariants.length > 0 ? "variants" : "start",
  );
  const [candidates, setCandidates] = useState<SolutionStrategy[]>([]);
  const [selected, setSelected] = useState<Record<string, "main" | "alternative">>({});
  const [variants, setVariants] = useState<ProblemSolutionVariant[]>(initialVariants);
  const [analysisSummary, setAnalysisSummary] = useState("");
  const [customText, setCustomText] = useState("");
  const [customValidation, setCustomValidation] = useState<StrategyValidationResult | null>(null);
  const [explanationGuidance, setExplanationGuidance] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [error, setError] = useState("");
  const [activeJobId, setActiveJobId] = useState<number | null>(null);
  const [activeJobMode, setActiveJobMode] = useState<string | null>(null);
  const operationRef = useRef(0);
  const mountedRef = useRef(true);
  const lastActionRef = useRef<(() => Promise<void>) | null>(null);

  useEffect(() => () => {
    mountedRef.current = false;
    operationRef.current += 1;
  }, []);

  const commit = (next: ProblemSolutionVariant[]) => {
    const ordered = orderedVariants(next);
    setVariants(ordered);
    onChange(ordered, composeVariantSolutions(ordered), composeVariantExplanations(ordered));
  };

  const runJob = async (
    conversionMode: string,
    inputText: string,
    options: Record<string, unknown>,
    stage: string,
  ): Promise<AiJob> => {
    const backgroundReviewable = conversionMode === "generate_strategy_explanation";
    setProgress(stage);
    setActiveJobMode(conversionMode);
    const job = await aiCreateJob({
      sourceType: "text",
      conversionMode,
      options: {
        solutionSubject,
        solutionLayout: "two_column",
        solutionDetail: "standard",
        useTemplateContext: true,
        hideFromHistory: !backgroundReviewable,
        ...options,
      },
      inputText,
      targetEntityType: "problem",
      targetEntityId: problem.id,
      targetField: backgroundReviewable ? "explanation_latex" : "solution_workflow",
    });
    if (mountedRef.current) setActiveJobId(job.id);
    let current = job;
    while (RUNNING.has(current.status)) {
      if (!mountedRef.current) throw new Error("処理を中止しました");
      if (mountedRef.current) setProgress(current.progressMessage || stage);
      await sleep(1200);
      current = await aiGetJob(current.id);
    }
    if (mountedRef.current) {
      setActiveJobId(null);
      setActiveJobMode(null);
    }
    if (current.status !== "completed") {
      throw new Error(current.errorMessage || `${stage}に失敗しました`);
    }
    return current;
  };

  const execute = async (action: () => Promise<void>) => {
    if (busy) return;
    lastActionRef.current = action;
    const operation = ++operationRef.current;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (cause) {
      if (operation === operationRef.current && mountedRef.current) setError(String(cause));
    } finally {
      if (operation === operationRef.current && mountedRef.current) {
        setBusy(false);
        setProgress("");
        setActiveJobId(null);
        setActiveJobMode(null);
      }
    }
  };

  const generateStrategies = async (): Promise<SolutionStrategy[]> => {
    const job = await runJob("solution_strategies", problem.statement_latex, {}, "問題を解析しています…");
    const result = workflowResult(job);
    const strategies = result.strategies ?? [];
    if (strategies.length === 0) throw new Error("解法候補を取得できませんでした");
    const analysis = result.analysis;
    if (analysis) {
      setAnalysisSummary([analysis.subject, analysis.problemType, ...analysis.cautions].filter(Boolean).join(" / "));
    }
    setCandidates(strategies);
    setSelected({ [strategies[0].id]: "main" });
    return strategies;
  };

  const generateSolutionPlan = async (strategy: SolutionStrategy): Promise<SolutionPlan> => {
    const job = await runJob(
      "solution_plan",
      problem.statement_latex,
      { selectedStrategy: strategy },
      `「${strategy.title}」の答案を構成しています…`,
    );
    const plan = workflowResult(job).plan;
    if (!plan) throw new Error("答案設計を取得できませんでした");
    return plan;
  };

  const generateSolution = async (
    strategy: SolutionStrategy,
    plan: SolutionPlan,
  ): Promise<string> => {
    const solutionJob = await runJob(
      "generate_strategy_solution",
      problem.statement_latex,
      { selectedStrategy: strategy, solutionPlan: plan },
      `「${strategy.title}」で試験答案を生成しています…`,
    );
    const blockingWarnings = solutionJob.warnings.filter((warning) => warning.severity === "error");
    if (blockingWarnings.length > 0) {
      throw new Error(`生成答案を安全に使用できません: ${blockingWarnings.map((warning) => warning.message).join(" / ")}`);
    }
    const solution = solutionJob.outputLatex.trim();
    if (!solution) throw new Error("試験答案を取得できませんでした");
    return solution;
  };

  const verifySolution = async (strategy: SolutionStrategy, solution: string): Promise<VerificationResult> => {
    const input = [
      "【問題文】",
      problem.statement_latex,
      "",
      "【検証対象の解答】",
      solution,
    ].join("\n");
    const job = await runJob(
      "solution_verification",
      input,
      { selectedStrategy: strategy },
      `「${strategy.title}」の数学的な誤りを確認しています…`,
    );
    const verification = workflowResult(job).verification;
    if (!verification) throw new Error("解答の検証結果を取得できませんでした");
    return verification;
  };

  const generateVariant = async (
    strategy: SolutionStrategy,
    role: "main" | "alternative",
    previous?: ProblemSolutionVariant,
  ): Promise<ProblemSolutionVariant> => {
    const plan = await generateSolutionPlan(strategy);
    let solution = await generateSolution(strategy, plan);
    let verification: VerificationResult;
    try {
      verification = await verifySolution(strategy, solution);
      if (verification.correctedSolution?.trim()) {
        solution = verification.correctedSolution.trim();
        verification = await verifySolution(strategy, solution);
        if (verification.correctedSolution?.trim()) solution = verification.correctedSolution.trim();
      }
    } catch (cause) {
      verification = {
        valid: false,
        issues: [{ severity: "warning", message: `検証処理に失敗しました。生成済み答案は保持されています: ${String(cause)}` }],
      };
    }
    const inheritedExplanation = previous?.explanation
      ?? (role === "main" && problem.explanation_latex.trim() ? problem.explanation_latex : undefined);
    return {
      id: previous?.id ?? `variant-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      strategy,
      role,
      plan,
      solution,
      solutionBlocks: blocksFromSolution(solution),
      verification,
      explanation: inheritedExplanation,
      explanationSections: previous?.explanationSections ?? [],
      explanationOutdated: !!inheritedExplanation,
    };
  };

  const generateMany = async (entries: Array<{ strategy: SolutionStrategy; role: "main" | "alternative" }>) => {
    const next: ProblemSolutionVariant[] = [];
    for (const entry of entries) {
      const previous = variants.find((variant) => variant.strategy.id === entry.strategy.id);
      next.push(await generateVariant(entry.strategy, entry.role, previous));
      commit(next);
    }
    commit(next);
    setScreen("variants");
  };

  const quickGenerate = async () => {
    const strategies = await generateStrategies();
    const best = strategies.find((strategy) => strategy.suitability?.examAnswer) ?? strategies[0];
    await generateMany([{ strategy: best, role: "main" }]);
  };

  const chooseStrategies = async () => {
    await generateStrategies();
    setScreen("candidates");
  };

  const generateSelected = async () => {
    const entries = candidates
      .filter((strategy) => selected[strategy.id])
      .map((strategy) => ({ strategy, role: selected[strategy.id] }));
    if (entries.length === 0) throw new Error("解法を1つ以上選択してください");
    if (!entries.some((entry) => entry.role === "main")) entries[0].role = "main";
    await generateMany(entries);
  };

  const validateCustom = async () => {
    if (!customText.trim()) throw new Error("解法の方針を入力してください");
    const input = ["【問題文】", problem.statement_latex, "", "【ユーザー指定解法】", customText.trim()].join("\n");
    const job = await runJob("solution_strategy_validation", input, {}, "指定された解法を確認しています…");
    const validation = workflowResult(job).validation;
    if (!validation) throw new Error("解法の確認結果を取得できませんでした");
    setCustomValidation(validation);
    if (validation.valid) {
      await generateMany([{ strategy: validation.normalizedStrategy, role: "main" }]);
    }
  };

  const generateExplanation = async (variant: ProblemSolutionVariant) => {
    const input = [
      "【問題文】",
      problem.statement_latex,
      "",
      "【参照する解答】",
      variant.solution,
    ].join("\n");
    const job = await runJob(
      "generate_strategy_explanation",
      input,
      {
        selectedStrategy: variant.strategy,
        explanationGuidance: explanationGuidance.trim(),
        backgroundWorkflowResult: "solution_explanation",
        solutionVariantId: variant.id,
        solutionSource: variant.solution,
      },
      `「${variant.strategy.title}」の確定済み解答に沿って解説を作成しています…`,
    );
    const explanation = job.outputLatex.trim();
    const blockingWarnings = job.warnings.filter((warning) => warning.severity === "error");
    if (blockingWarnings.length > 0) {
      throw new Error(`生成解説を安全に使用できません: ${blockingWarnings.map((warning) => warning.message).join(" / ")}`);
    }
    if (!explanation) throw new Error("解説を取得できませんでした");
    const blocks = variant.solutionBlocks?.length ? variant.solutionBlocks : blocksFromSolution(variant.solution);
    commit(variants.map((item) => item.id === variant.id ? {
      ...item,
      explanation,
      explanationOutdated: false,
      explanationSections: [{ solutionBlockIds: blocks.map((block) => block.id), content: explanation }],
    } : item));
  };

  const regenerateSolution = async (variant: ProblemSolutionVariant) => {
    const regenerated = await generateVariant(variant.strategy, variant.role, variant);
    commit(variants.map((item) => item.id === variant.id ? regenerated : item));
  };

  const cancelCurrent = async () => {
    operationRef.current += 1;
    if (activeJobId !== null) await aiCancelJob(activeJobId).catch(() => undefined);
    setBusy(false);
    setProgress("");
    setActiveJobId(null);
    setActiveJobMode(null);
    setError("処理を中止しました。完了済みの解答は保持されています。");
  };

  const closeDialog = () => {
    if (busy && activeJobMode === "generate_strategy_explanation") {
      showToast(
        activeJobId === null
          ? "解説生成をバックグラウンドへ移しました。完了後にAI一覧から確認できます"
          : `解説生成 #${activeJobId}をバックグラウンドへ移しました。完了後にAI一覧から確認できます`,
      );
      onClose();
      return;
    }
    operationRef.current += 1;
    if (activeJobId !== null) void aiCancelJob(activeJobId).catch(() => undefined);
    onClose();
  };

  const setStrategyRole = (strategyId: string, role: "main" | "alternative") => {
    setSelected((current) => {
      const next = { ...current, [strategyId]: role };
      if (role === "main") {
        for (const id of Object.keys(next)) if (id !== strategyId && next[id] === "main") next[id] = "alternative";
      }
      return next;
    });
  };

  const renderStart = () => (
    <div className="space-y-3">
      <p className="text-sm">生成方法を選択してください。どのモードでも内部では解法を確定してから答案を設計します。</p>
      <button className="card w-full p-4 text-left" disabled={busy} onClick={() => execute(quickGenerate)}>
        <div className="flex items-center gap-2 font-bold"><Icon name="sparkle" size={16} /> クイック生成</div>
        <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>AIが最適と判断した標準解法ですぐに答案を生成します。</p>
      </button>
      <button className="card w-full p-4 text-left" disabled={busy} onClick={() => execute(chooseStrategies)}>
        <div className="font-bold">解法を選んで生成</div>
        <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>本質的に異なる複数の解法候補から、主解法・別解を選べます。</p>
      </button>
      <button className="card w-full p-4 text-left" disabled={busy} onClick={() => { setScreen("custom"); setCustomValidation(null); }}>
        <div className="font-bold">解法を指定して生成</div>
        <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>自然言語で方針を入力し、成立性を確認してから答案化します。</p>
      </button>
    </div>
  );

  const renderCandidates = () => (
    <div className="space-y-3">
      <div>
        <h3 className="text-sm font-bold">解法を選択</h3>
        {analysisSummary && <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>{analysisSummary}</p>}
      </div>
      {candidates.map((strategy) => (
        <StrategyCard
          key={strategy.id}
          strategy={strategy}
          selected={!!selected[strategy.id]}
          role={selected[strategy.id] ?? "alternative"}
          onToggle={() => setSelected((current) => {
            if (current[strategy.id]) {
              const next = { ...current };
              delete next[strategy.id];
              if (!Object.values(next).includes("main")) {
                const first = Object.keys(next)[0];
                if (first) next[first] = "main";
              }
              return next;
            }
            return { ...current, [strategy.id]: Object.keys(current).length === 0 ? "main" : "alternative" };
          })}
          onRole={(role) => setStrategyRole(strategy.id, role)}
        />
      ))}
      <button className="btn btn-outline btn-sm" onClick={() => { setScreen("custom"); setCustomValidation(null); }}>＋ 自分で解法を指定</button>
      <div className="flex justify-between gap-2 border-t pt-3" style={{ borderColor: "var(--border)" }}>
        <button className="btn btn-ghost" onClick={() => setScreen("start")}>戻る</button>
        <button className="btn btn-solid" disabled={busy || Object.keys(selected).length === 0} onClick={() => execute(generateSelected)}>この解法で答案を生成</button>
      </div>
    </div>
  );

  const renderCustom = () => (
    <div className="space-y-3">
      <div>
        <h3 className="text-sm font-bold">自分で解法を指定</h3>
        <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>説明不足はAIが意図を補います。数学的に成立するか確認してから答案を生成します。</p>
      </div>
      <textarea
        className="input min-h-32 w-full resize-y text-sm"
        value={customText}
        onChange={(event) => { setCustomText(event.target.value); setCustomValidation(null); }}
        placeholder="例: f'(x)の不等式を先に証明し、それを積分する方法"
        maxLength={2000}
      />
      {customValidation && !customValidation.valid && (
        <div className="rounded border p-3" style={{ borderColor: "rgba(251,191,36,0.5)", background: "var(--warn-dim)" }}>
          <p className="text-sm font-semibold">その方針では最後まで解くのが難しい可能性があります</p>
          <p className="mt-1 text-xs whitespace-pre-wrap">{customValidation.message}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button className="btn btn-outline btn-sm" onClick={() => execute(() => generateMany([{ strategy: fallbackCustomStrategy(customText), role: "main" }]))}>そのまま試す</button>
            <button className="btn btn-solid btn-sm" onClick={() => execute(() => generateMany([{ strategy: customValidation.suggestedStrategy, role: "main" }]))}>AIによる修正案で生成</button>
            <button className="btn btn-ghost btn-sm" onClick={() => setScreen(candidates.length ? "candidates" : "start")}>解法候補に戻る</button>
          </div>
        </div>
      )}
      <div className="flex justify-between gap-2 border-t pt-3" style={{ borderColor: "var(--border)" }}>
        <button className="btn btn-ghost" onClick={() => setScreen(candidates.length ? "candidates" : "start")}>戻る</button>
        <button className="btn btn-solid" disabled={busy || !customText.trim()} onClick={() => execute(validateCustom)}>成立性を確認して答案を生成</button>
      </div>
    </div>
  );

  const renderVariants = () => (
    <div className="space-y-4">
      <div className="rounded border p-3" style={{ borderColor: "var(--border)", background: "var(--panel-2)" }}>
        <label className="section-label mb-1 block">解説の追加指示（任意）</label>
        <input
          className="input w-full text-xs"
          value={explanationGuidance}
          onChange={(event) => setExplanationGuidance(event.target.value)}
          placeholder="もっと詳しく / 初学者向け / この式変形を詳しく / 図形的意味も追加"
          maxLength={1000}
        />
        <p className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>どの指定でも、確定済み解答と異なる解法には切り替えません。</p>
      </div>
      {orderedVariants(variants).map((variant, index) => (
        <article key={variant.id} className="card p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="badge badge-muted">{variant.role === "main" ? "主解法" : `別解${index}`}</span>
            <h3 className="min-w-0 flex-1 text-sm font-bold">{variant.strategy.title}</h3>
            {variant.verification?.valid ? (
              <span className="badge" style={{ color: "var(--success)", borderColor: "rgba(197,183,223,0.4)" }}>検証済み</span>
            ) : (
              <span className="badge badge-warn">要確認</span>
            )}
            {variant.explanationOutdated && <span className="badge badge-warn">解説の再生成を推奨</span>}
          </div>
          <p className="mt-1 text-xs" style={{ color: "var(--muted)" }}>{variant.strategy.summary}</p>
          <label className="section-label mb-1 mt-3 block">試験答案（編集可能）</label>
          <textarea
            className="input min-h-48 w-full resize-y font-mono text-xs leading-5"
            value={variant.solution}
            onChange={(event) => {
              const solution = event.target.value;
              commit(variants.map((item) => item.id === variant.id ? {
                ...item,
                solution,
                solutionBlocks: blocksFromSolution(solution),
                verification: undefined,
                explanationOutdated: !!item.explanation,
              } : item));
            }}
          />
          {!!variant.verification?.issues.length && (
            <div className="mt-2 space-y-1">
              {variant.verification.issues.map((issue, issueIndex) => (
                <p key={`${issue.message}-${issueIndex}`} className="text-[11px]" style={{ color: issue.severity === "error" ? "var(--danger)" : "var(--warn)" }}>
                  {issue.location ? `${issue.location}: ` : ""}{issue.message}
                </p>
              ))}
            </div>
          )}
          <div className="mt-2 flex flex-wrap justify-end gap-2">
            <button className="btn btn-outline btn-sm" disabled={busy} onClick={() => execute(() => regenerateSolution(variant))}>解答を再生成</button>
            <button className="btn btn-solid btn-sm" disabled={busy || !variant.solution.trim()} onClick={() => execute(() => generateExplanation(variant))}>
              {variant.explanation ? "解説を再生成" : "この解答から解説を生成"}
            </button>
          </div>
          {variant.explanation !== undefined && (
            <>
              <label className="section-label mb-1 mt-3 block">解説（編集可能）</label>
              <textarea
                className="input min-h-40 w-full resize-y font-mono text-xs leading-5"
                value={variant.explanation}
                onChange={(event) => commit(variants.map((item) => item.id === variant.id ? {
                  ...item,
                  explanation: event.target.value,
                  explanationOutdated: false,
                  explanationSections: [{
                    solutionBlockIds: (item.solutionBlocks ?? []).map((block) => block.id),
                    content: event.target.value,
                  }],
                } : item))}
              />
            </>
          )}
        </article>
      ))}
      <div className="flex flex-wrap justify-between gap-2 border-t pt-3" style={{ borderColor: "var(--border)" }}>
        <button className="btn btn-outline" disabled={busy} onClick={() => execute(chooseStrategies)}>解法候補を選び直す</button>
        <button className="btn btn-solid" onClick={closeDialog}>
          {busy && activeJobMode === "generate_strategy_explanation"
            ? "バックグラウンドへ"
            : "問題へ反映して閉じる"}
        </button>
      </div>
    </div>
  );

  return (
    <Modal title="AI解答作成 — 解法を確定してから答案・解説を生成" onClose={closeDialog} wide>
      {busy && (
        <div className="mb-3 rounded border p-3" style={{ borderColor: "var(--accent)", background: "var(--accent-dim)" }}>
          <div className="flex items-center gap-2">
            <span className="h-4 w-4 animate-spin rounded-full border-2 border-t-transparent" style={{ borderColor: "var(--accent)", borderTopColor: "transparent" }} />
            <p className="min-w-0 flex-1 text-sm font-semibold">{progress || "AI処理を実行しています…"}</p>
            {activeJobMode === "generate_strategy_explanation" && (
              <button className="btn btn-outline btn-sm" onClick={closeDialog}>バックグラウンドへ</button>
            )}
            <button className="btn btn-ghost btn-sm" onClick={() => void cancelCurrent()}>中止</button>
          </div>
        </div>
      )}
      {error && (
        <div className="mb-3 rounded border p-3" style={{ borderColor: "rgba(241,106,117,0.5)", background: "var(--danger-dim)" }}>
          <p className="text-sm whitespace-pre-wrap" style={{ color: "var(--danger)" }}>{error}</p>
          <div className="mt-2 flex flex-wrap gap-2">
            {lastActionRef.current && <button className="btn btn-outline btn-sm" disabled={busy} onClick={() => execute(lastActionRef.current!)}>再試行</button>}
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => execute(quickGenerate)}>クイック生成</button>
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setScreen("custom")}>解法を自分で指定</button>
          </div>
        </div>
      )}
      {screen === "start" && renderStart()}
      {screen === "candidates" && renderCandidates()}
      {screen === "custom" && renderCustom()}
      {screen === "variants" && renderVariants()}
    </Modal>
  );
}
