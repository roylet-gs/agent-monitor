import React, { useState, useEffect } from "react";
import { Text } from "ink";

const BASE_RGB: Record<string, [number, number, number]> = {
  green: [0x8B, 0xBD, 0x58],
  cyan: [0, 255, 255],
  yellow: [255, 255, 0],
  red: [255, 0, 0],
  blueBright: [100, 149, 237],
  gray: [128, 128, 128],
};

const STEPS = 24;
const INTERVAL = 100; // ~2.4s full cycle

function toHex(r: number, g: number, b: number): string {
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

const GREY: [number, number, number] = [100, 100, 100];

function lerp(a: number, b: number, t: number): number {
  return Math.round(a + (b - a) * t);
}

function buildGradient(base: [number, number, number]): string[] {
  const colors: string[] = [];
  for (let i = 0; i < STEPS; i++) {
    // triangle wave: 0 (full color) → 1 (grey) → 0 (full color)
    const half = STEPS / 2;
    const t = i < half ? i / half : 2 - i / half;
    colors.push(
      toHex(
        lerp(base[0], GREY[0], t),
        lerp(base[1], GREY[1], t),
        lerp(base[2], GREY[2], t),
      ),
    );
  }
  return colors;
}

const gradientCache = new Map<string, string[]>();

function getGradient(color: string): string[] {
  let gradient = gradientCache.get(color);
  if (!gradient) {
    const base = BASE_RGB[color] ?? [0, 255, 0];
    gradient = buildGradient(base);
    gradientCache.set(color, gradient);
  }
  return gradient;
}

interface PulsingDotProps {
  color: string;
}

export function PulsingDot({ color }: PulsingDotProps) {
  const [step, setStep] = useState(0);
  const gradient = getGradient(color);

  useEffect(() => {
    const timer = setInterval(() => {
      setStep((s) => (s + 1) % gradient.length);
    }, INTERVAL);
    return () => clearInterval(timer);
  }, [gradient]);

  return <Text color={gradient[step]}>●</Text>;
}
