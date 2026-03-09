import { useState, useEffect } from "react";

const STEPS = 12;
const INTERVAL = 200;

type Listener = (step: number) => void;

let globalStep = 0;
let listeners = new Set<Listener>();
let timer: ReturnType<typeof setInterval> | null = null;

function startTimer() {
  if (timer) return;
  timer = setInterval(() => {
    globalStep = (globalStep + 1) % STEPS;
    for (const fn of listeners) fn(globalStep);
  }, INTERVAL);
}

function stopTimer() {
  if (timer && listeners.size === 0) {
    clearInterval(timer);
    timer = null;
  }
}

/** Reset global state — for tests only */
export function _resetAnimationTimer() {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
  globalStep = 0;
  listeners.clear();
}

export function useAnimationStep(): number {
  const [step, setStep] = useState(globalStep);

  useEffect(() => {
    listeners.add(setStep);
    startTimer();
    return () => {
      listeners.delete(setStep);
      stopTimer();
    };
  }, []);

  return step;
}
