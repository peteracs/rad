"""Exact independent bounds for the finite-transfer closure audit."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess

HERE = Path(__file__).resolve().parent

def main():
    delta,T = F(1,2**18),F(1,2**98)
    high_upper = delta**2/(2*4**3)
    assert high_upper == F(1,2**43)
    assert 24576 > F(313,2)**2 and delta < F(1,2)
    total_lower = F(156**2,2)
    assert total_lower > 2**13
    assert high_upper/total_lower < F(1,2**56) < F(1,2)
    assert (2**49*T)/(1024*T) == 2**39
    assert (F(3,64)*T)/(1024*T) == F(3,65536) < F(1,10000)
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'transfer_closure.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[8:] for line in run.stdout.splitlines() if line.startswith('closure ')))
    assert row['high_fraction_upper_negative_power_two'] == 56
    assert row['whole_error_to_signal_upper_power_two'] == 39
    assert row['half_energy_handoff_excluded']
    assert not row['full_new_profile_relative_error_certified'] and not row['repeatable_cascade_proved']
    assert 'resolver `CheckClosure`' in run.stdout and 'law `SubmitClosure`' in run.stdout
    for mutation in ['forge-profile','forge-repeat']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged next-scale closure' in bad.stdout+bad.stderr
    print('PASS: weighted high-band energy upper bound, transfer fraction, whole/projected relative-error bounds and WHY provenance.')
    print('PASS: forged profile/iteration rejected. No repeatable cascade or blowup established.')

if __name__ == '__main__':
    main()
