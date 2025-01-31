/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {eventWithTime, playerConfig} from 'rrweb';

import React, {useEffect} from 'react';
import {useRef} from 'react';
import {Replayer} from '@rrweb/replay';

import '../css/RrwebPlayer.css';
import '@rrweb/replay/dist/style.css';

type RrwebPlayerProps = {
  events: eventWithTime[];
  /** Replay (loop) after the given milliseconds. Set to `undefined` to disable auto replay.  */
  autoReplayAfter?: number;
} & Partial<playerConfig>;

const defaultPlayerConfig: Partial<RrwebPlayerProps> = {
  autoReplayAfter: 5000,
  mouseTail: {
    strokeStyle: 'rgba(128, 128, 128, 0.5)',
  },
  // too noisy
  showWarning: false,
};

/**
 * React wrapper around rrweb-player.
 * See https://github.com/rrweb-io/rrweb/blob/master/packages/rrweb-player/README.md
 */
export default function RrwebPlayer(props: RrwebPlayerProps) {
  const divRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<Replayer | null>(null);
  const progressRef = useRef<HTMLDivElement>(null);
  // 'auto': auto play on visible; 'manual': not affected by visibility
  const playControlRef = useRef<'manual' | 'auto'>('auto');

  const isPlaying = (): boolean => {
    return playerRef.current?.service.state.matches('playing');
  }
  const setPlaying = (playing: boolean) => {
    const player = playerRef.current;
    if (player == null) {
      return;
    }
    if (playing) {
      const totalTime = player.getMetaData().totalTime;
      const time = Math.max(0, player.getCurrentTime());
      player.play(time >= totalTime ? 0 : time);
    } else {
      player.pause();
    }
  }

  useEffect(() => {
    const target = divRef.current;
    if (target == null) {
      return;
    }
    const {events, autoReplayAfter, ...rest} = {...defaultPlayerConfig, ...props};
    let player = playerRef.current = playerRef.current ?? new Replayer(events, {root: target, ...defaultPlayerConfig, ...rest});

    // Start playing when scrolled into view.
    const observer = new IntersectionObserver((entries) => {
      if (playControlRef.current === 'manual') {
        return;
      }
      setPlaying(entries[0].isIntersecting);
    }, {
      threshold: 0.6,
      root: null,
    });

    // Auto replay (loop) on end.
    if (autoReplayAfter != null) {
      player.on('finish', () => {
        player?.pause();
        playControlRef.current = 'auto';
        const pos = player?.getCurrentTime();
        setTimeout(() => {
          if (player?.getCurrentTime() === pos) {
            player?.play(0);
          }
        }, autoReplayAfter);
      });
    }

    // Update progress bar (only visible on hover).
    const totalTime = player.getMetaData().totalTime;
    let progressTimer = undefined;
    const updateProgress = () => {
      const progressElement = progressRef.current;
      if (progressElement && player) {
        const currentTime = Math.max(0, player.getCurrentTime() ?? 0);
        const progress = Math.floor(Math.min(currentTime / totalTime, 1) * player.wrapper.clientWidth);
        const width = `${progress}px`;
        if (progressElement.style.width !== width) {
          progressElement.style.width = width;
        }
      }
      progressTimer = requestAnimationFrame(updateProgress);
    };
    progressTimer = requestAnimationFrame(updateProgress);

    // Play if visible.
    observer.observe(target);

    return () => {
      observer.disconnect();
      progressTimer && cancelAnimationFrame(progressTimer);
      player?.destroy();
      player = playerRef.current = undefined;
    };
  }, []);

  // Click to toggle play/pause.
  const handleClick = () => {
    playControlRef.current = 'manual';
    setPlaying(!isPlaying());
  };

  return (
    <div className='rr-replay-container'>
      <div ref={divRef} style={{height: 'fit-content', width: 'fit-content', clear: 'both'}} onClick={handleClick} />
      <div className='rr-progress-container'>
        <div className='rr-progress-inner' ref={progressRef} />
      </div>
    </div>
  );
}
