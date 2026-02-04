/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useEffect, useState, useCallback} from 'react';

import './Confetti.css';

type Particle = {
  id: number;
  x: number;
  color: string;
  delay: number;
  duration: number;
  rotation: number;
  scale: number;
};

const COLORS = [
  '#22c55e', // green (success)
  '#3b82f6', // blue
  '#f59e0b', // amber
  '#ec4899', // pink
  '#8b5cf6', // purple
  '#06b6d4', // cyan
];

/**
 * Confetti celebration effect.
 * Listens for 'isl-confetti' custom event on window.
 */
export function Confetti() {
  const [particles, setParticles] = useState<Particle[]>([]);

  const triggerConfetti = useCallback(() => {
    const newParticles: Particle[] = [];
    const particleCount = 50;

    for (let i = 0; i < particleCount; i++) {
      newParticles.push({
        id: Date.now() + i,
        x: Math.random() * 100, // % position across screen
        color: COLORS[Math.floor(Math.random() * COLORS.length)],
        delay: Math.random() * 0.3,
        duration: 1.5 + Math.random() * 1,
        rotation: Math.random() * 360,
        scale: 0.5 + Math.random() * 0.5,
      });
    }

    setParticles(newParticles);

    // Clean up after animation completes
    setTimeout(() => {
      setParticles([]);
    }, 3000);
  }, []);

  useEffect(() => {
    const handler = () => triggerConfetti();
    window.addEventListener('isl-confetti', handler);
    return () => window.removeEventListener('isl-confetti', handler);
  }, [triggerConfetti]);

  if (particles.length === 0) {
    return null;
  }

  return (
    <div className="confetti-container" aria-hidden="true">
      {particles.map(particle => (
        <div
          key={particle.id}
          className="confetti-particle"
          style={{
            left: `${particle.x}%`,
            backgroundColor: particle.color,
            animationDelay: `${particle.delay}s`,
            animationDuration: `${particle.duration}s`,
            transform: `rotate(${particle.rotation}deg) scale(${particle.scale})`,
          }}
        />
      ))}
    </div>
  );
}
