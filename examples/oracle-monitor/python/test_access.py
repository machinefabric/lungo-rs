"""The oracle-monitor example from Python: a hand-written evaluator tested against the proved
oracle, and the proved monitor fed a log of events."""

import itertools
import random
import unittest

import access as A
import lungo_py

ROLES = ["admin", "editor", "viewer"]
ACTIONS = ["read", "write", "delete"]


def permits(policy, role, action):
    """The hand-written evaluator: an allowing rule for the pair, and no denying one."""
    effects = {type(r.effect) for r in policy if r.role == role and r.action == action}
    return A.EffectAllow in effects and A.EffectDeny not in effects


class AccessTest(unittest.TestCase):
    # TEST0333: the hand-written Python evaluator agrees with the proved oracle
    def test_0333_the_hand_written_python_evaluator_agrees_with_the_proved_oracle(self):
        rng = random.Random(333)
        for _ in range(200):
            policy = [
                A.Rule(rng.choice(ROLES), rng.choice(ACTIONS), A.EffectDeny() if rng.random() < 0.25 else A.EffectAllow())
                for _ in range(rng.randrange(8))
            ]
            for role, action in itertools.product(ROLES, ACTIONS):
                self.assertEqual(permits(policy, role, action), A.evaluate(policy, A.Request(role, action)), (policy, role, action))
        self.assertEqual([r.role for r in A.ASSURANCE.roles], ["lungo.oracle"])

    # TEST0334: the proved monitor, fed a log of events from Python
    def test_0334_the_proved_monitor_fed_a_log_of_events_from_python(self):
        policy = [A.Rule("editor", "write", A.EffectAllow()), A.Rule("viewer", "read", A.EffectAllow())]
        log = """
            sign-in ada editor
            access ada write
            sign-in bo viewer
            access bo read
            access bo write
            sign-out ada
        """
        session = A.start()
        violations = []
        for line in log.split("\n"):
            words = line.split()
            if not words:
                continue
            event = {
                "sign-in": lambda w: A.EventSignIn(w[0], w[1]),
                "access": lambda w: A.EventAccess(w[0], w[1]),
                "sign-out": lambda w: A.EventSignOut(w[0]),
            }[words[0]](words[1:])
            verdict = A.observe(policy, session, event)
            if isinstance(verdict, lungo_py.Ok):
                session = verdict.value
            else:
                violations.append(verdict.error)
        self.assertEqual(violations, ["the policy does not permit bo to write"])
        self.assertEqual([u for u, _ in session.users], ["bo"])
        claim = A.ASSURANCE.claim("Access.observe_sound")
        self.assertEqual((claim.relation, claim.status), ("lungo.monitors", "proved"))


if __name__ == "__main__":
    unittest.main()
