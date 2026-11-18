def count_words(text):
    return len(text.split())


def longest_word(text):
    best = None
    for word in text.split():
        if best is None or len(word) > len(best):
            best = word
    return best
