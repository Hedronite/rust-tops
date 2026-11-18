from wc import count_words, longest_word


def test_empty():
    assert count_words("") == 0
    assert longest_word("") is None


def test_counts_words():
    assert count_words("a  b\tc\n") == 3


class TestLongest:
    def test_first_of_a_tie_wins(self):
        assert longest_word("ab cd e") == "ab"
