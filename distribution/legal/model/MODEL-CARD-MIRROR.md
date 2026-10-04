---
license: apache-2.0
datasets:
- OleehyO/latex-formulas
metrics:
- bleu
pipeline_tag: image-to-text
tags:
- ocr
- image to latex
---

[中文版本](./README_zh.md)

# About TexTeller

* 📮[2024-03-25] TexTeller 2.0 released! The training data for TexTeller 2.0 has been increased to 7.5M (about **15 times more** than TexTeller 1.0 and also improved in data quality). The trained TexTeller 2.0 demonstrated **superior performance** in the test set, especially in recognizing rare symbols, complex multi-line formulas, and matrices.
    > [There](https://github.com/OleehyO/TexTeller/blob/main/assets/test.pdf) are more test images here and a horizontal comparison of recognition models from different companies.

TexTeller is a ViT-based model designed for end-to-end formula recognition. It can recognize formulas in natural images and convert them into LaTeX-style formulas.

TexTeller is trained on a larger dataset of image-formula pairs (a 550K dataset available [here](https://huggingface.co/datasets/OleehyO/latex-formulas)), **exhibits superior generalization ability and higher accuracy compared to [LaTeX-OCR](https://github.com/lukas-blecher/LaTeX-OCR)**, which uses approximately 100K data points. This larger dataset enables TexTeller to cover most usage scenarios more effectively.

> For more details, please refer to the 𝐓𝐞𝐱𝐓𝐞𝐥𝐥𝐞𝐫 [GitHub repository](https://github.com/OleehyO/TexTeller?tab=readme-ov-file).