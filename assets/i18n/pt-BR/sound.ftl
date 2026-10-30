# Sons e anúncio de lances (spec M8-11)
menu-view-sounds = Sons...
sound-title = Sons
sound-enabled = Tocar sons
sound-volume = Volume
sound-on-new = Som a cada lance novo
sound-on-forward = Som ao avançar
sound-on-back = Som ao voltar
sound-tick-only = Mesmo tique para todo lance
sound-announce = Falar o lance (precisa dos arquivos de palavras na pasta)
sound-folder = Pasta de sons
sound-folder-builtin = Somente os sons embutidos
sound-folder-choose = Escolher...
sound-folder-remove = Remover
sound-folder-none = Sem pasta: usam-se os sons embutidos.
sound-folder-loading = Lendo a pasta...
sound-folder-found = { $found ->
    [one] 1 arquivo de som encontrado
   *[other] { $found } arquivos de som encontrados
}
sound-folder-skipped = { $skipped ->
    [one] 1 arquivo ignorado (WAV inválido)
   *[other] { $skipped } arquivos ignorados (WAV inválido)
}
sound-folder-failed = Não foi possível ler a pasta.
sound-folder-help = Arquivos como no Scid: King.wav, Knight.wav, a.wav a h.wav, 1.wav a 8.wav, x.wav, Check.wav, Mate.wav, Promote.wav, CastleK.wav, CastleQ.wav, Back.wav, move.wav, start.wav, end.wav.
sound-last = Último anúncio: { $text }
sound-test = Testar
sound-close = Fechar
sound-pack = Pacote de sons
sound-pack-default = Padrão (sintetizado)
sound-pack-user = { $name } (do usuário)
sound-pack-author = Autor: { $author }
sound-pack-license = Licença: { $license }
sound-pack-covered = { $count ->
    [one] 1 de { $total } sons; o resto usa o som padrão
   *[other] { $count } de { $total } sons; o resto usa o som padrão
}
sound-pack-complete = O pacote tem os { $total } sons
sound-pack-loading = Carregando...
sound-pack-error-missing = O pacote de sons não foi encontrado.
sound-pack-error-invalid = O nome do pacote de sons não é válido.
sound-pack-error-io = Não foi possível ler o pacote de sons.
sound-pack-error = Não foi possível carregar o pacote de sons.
sound-pack-warnings = { $count ->
    [one] 1 aviso sobre este pacote (os sons afetados usam o som padrão)
   *[other] { $count } avisos sobre este pacote (os sons afetados usam o som padrão)
}
sound-packs-ignored = { $count ->
    [one] 1 item da pasta de sons foi ignorado
   *[other] { $count } itens da pasta de sons foram ignorados
}
sound-open-folder = Abrir pasta de sons
sound-user-folder-hint = Copie os pacotes de sons (uma pasta para cada) para esta pasta:
sound-advanced = Avançado
