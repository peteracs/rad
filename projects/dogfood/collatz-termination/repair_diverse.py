"""Seed the RAD campaign from several matrix families, not one control."""

import argparse
import random

from repair_campaign import request, run


def configuration(rounds=16, trials=20000):
    lanes = []
    for d in (2, 3, 4, 6, 8):
        for reverse in (False, True):
            for target in ((1, 2, 3) if reverse else (256, 512, 1024, 1792)):
                for style, maximum in enumerate((1, 2, 3, 7, 15, 31)):
                    req = request(d, reverse, target, trials=trials)
                    seed = req['seed'] + 7919 * (style + 1)
                    rng = random.Random(seed)
                    matrices = []
                    for symbol in range(7):
                        matrix = [[0] * (d + 1) for _ in range(d + 1)]
                        matrix[d][d] = 1
                        for i in range(d):
                            for j in range(d + 1):
                                if symbol == req['ground'] and j < d: continue
                                if style == 0: value = 0
                                elif style == 1: value = int(i == j or j == d)
                                elif style == 2: value = rng.randrange(2)
                                elif style == 3: value = rng.randrange(maximum + 1) if i <= j else 0
                                elif style == 4: value = rng.randrange(maximum + 1) if rng.randrange(4) == 0 else 0
                                else: value = rng.randrange(maximum + 1)
                                matrix[i][j] = value
                        matrices.append(matrix)
                    req.update(matrices=matrices, maximum=maximum, seed=seed,
                               strict_weight=(1, 8, 128, 1024, 128, 1024)[style],
                               temperature=(1, 8, 64, 128, 1024, 8192)[style])
                    lanes.append(dict(label=f'diverse-d{d}-r{int(reverse)}-s{target}-v{style}',
                                      omitted=-1, request=req))
    return dict(rounds=rounds, lanes=lanes)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--rounds', type=int, default=16)
    parser.add_argument('--trials', type=int, default=20000)
    args = parser.parse_args()
    run(configuration(args.rounds, args.trials), 'repair-diverse')
