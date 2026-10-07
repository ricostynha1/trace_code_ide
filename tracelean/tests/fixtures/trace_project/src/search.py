import functools


@functools.lru_cache
# @implements REQ-SEARCH-02
def search(index, query):
    return [x for x in index if query in x]
